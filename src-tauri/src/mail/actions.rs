//! Message actions — read flags and folder moves (trash, archive, any
//! folder). Shared by the Tauri commands and anything else that acts on mail
//! for the user: each updates the local cache, tells the windows
//! (`messages-changed`) and brings the server in line.

use tauri::{AppHandle, Emitter};

use crate::auth::Credential;
use crate::error::AppError;
use crate::models::{Account, MessagesChanged};
use crate::state::AppState;
use crate::timing;
use crate::{mail, storage};

/// Mark one message read/unread. Updates the local cache and notifies the UI
/// immediately, then pushes the `\Seen` flag to the server in the background —
/// opening a message never waits on the network for the dot to clear.
pub async fn set_message_read(
    app: &AppHandle,
    state: &AppState,
    message_id: i64,
    read: bool,
) -> Result<(), AppError> {
    let loc = storage::messages::location(&state.pool, message_id).await?;
    storage::messages::set_read(&state.pool, message_id, read).await?;
    // why the ids rather than a plain reload: the window patches this row in
    // place instead of re-running the whole list query for one dot.
    app.emit(
        "messages-changed",
        MessagesChanged::read(loc.account_id, vec![message_id], read),
    )?;

    // why: fetch account + credential on the command path (cheap, from the
    // session cache) so the spawned task owns everything it needs.
    let account = storage::accounts::get(&state.pool, loc.account_id).await?;
    let credential = state.credential(&account).await?;
    tauri::async_runtime::spawn(async move {
        // why: best effort — if the server STORE fails, the next sync's
        // reconcile adopts the server's flag, so nothing drifts permanently.
        if let Err(err) = push_seen_flag(&account, &credential, &loc.mailbox, loc.uid, read).await {
            eprintln!("failed to push read={read} for message {message_id}: {err}");
        }
    });
    Ok(())
}

/// Mark a whole selection read or unread. Same shape as set_message_read —
/// the cache and the UI update at once, the server catches up in the
/// background — but batched, so each account folder is one STORE.
pub async fn set_messages_read(
    app: &AppHandle,
    state: &AppState,
    message_ids: &[i64],
    read: bool,
    threads: bool,
) -> Result<(), AppError> {
    let batches = locate_batches(state, message_ids, threads).await?;
    for batch in &batch_ids(&batches) {
        storage::messages::set_read(&state.pool, *batch, read).await?;
    }
    for batch in &batches {
        let ids: Vec<i64> = batch.rows.iter().map(|(id, _)| *id).collect();
        app.emit(
            "messages-changed",
            MessagesChanged::read(batch.account_id, ids, read),
        )?;
    }

    for batch in batches {
        let account = storage::accounts::get(&state.pool, batch.account_id).await?;
        let credential = state.credential(&account).await?;
        let uids: Vec<i64> = batch.rows.iter().map(|(_, uid)| *uid).collect();
        tauri::async_runtime::spawn(async move {
            // why: best effort, like the single-message path — a failed STORE
            // is adopted back from the server by the next sync's reconcile.
            if let Err(err) =
                push_seen_flags(&account, &credential, &batch.mailbox, &uids, read).await
            {
                eprintln!("failed to push read={read} for {} rows: {err}", uids.len());
            }
        });
    }
    Ok(())
}

/// Every message row id across a set of batches.
fn batch_ids(batches: &[FolderBatch]) -> Vec<i64> {
    batches
        .iter()
        .flat_map(|batch| batch.rows.iter().map(|(id, _)| *id))
        .collect()
}

/// Move one message to the account's Trash folder, then drop it from the
/// local cache.
pub async fn move_to_trash(
    app: &AppHandle,
    state: &AppState,
    message_id: i64,
) -> Result<(), AppError> {
    move_to_special_folder(app, state, message_id, &["trash"], "trash").await
}

/// Archive one message: move it to the account's Archive folder, then drop it
/// from the local cache. Same server-confirmed path as trashing.
///
/// why the "all" fallback: Gmail has no \Archive folder — archiving means
/// removing the Inbox label, which over IMAP is a move into "All Mail" (\All).
pub async fn archive_message(
    app: &AppHandle,
    state: &AppState,
    message_id: i64,
) -> Result<(), AppError> {
    move_to_special_folder(app, state, message_id, &["archive", "all"], "archive").await
}

/// Move one message to a user-chosen folder of its account, then drop it
/// from the local cache. The destination must be a folder discovery has
/// mirrored — an unknown name fails before any IMAP command runs.
pub async fn move_message(
    app: &AppHandle,
    state: &AppState,
    message_id: i64,
    mailbox: &str,
) -> Result<(), AppError> {
    let loc = storage::messages::location(&state.pool, message_id).await?;
    if !storage::mailboxes::exists(&state.pool, loc.account_id, mailbox).await? {
        return Err(AppError::Imap(format!("no folder named {mailbox}")));
    }
    let rows = vec![(message_id, loc.uid)];
    move_rows_to_mailbox(app, state, loc.account_id, &loc.mailbox, &rows, mailbox).await
}

/// One IMAP session's worth of work: the rows of a single account's folder.
#[derive(Debug, PartialEq, Eq)]
struct FolderBatch {
    account_id: i64,
    mailbox: String,
    /// `(message row id, server uid)` — what move_rows_to_mailbox takes.
    rows: Vec<(i64, i64)>,
}

/// Split located messages so each batch is one account's folder.
///
/// why: a bulk action over a unified-inbox selection can span accounts and
/// folders, and a MOVE only applies to the mailbox that is SELECTed. Grouping
/// first means one connection per folder instead of one per message.
///
/// Batch order is (account, folder name) and rows keep their input order, so
/// a run is reproducible and a partial failure is explainable. A row named
/// more than once — conversation expansion can do that — is kept once.
fn group_by_folder(located: Vec<(i64, storage::messages::MessageLocation)>) -> Vec<FolderBatch> {
    let mut batches: std::collections::BTreeMap<(i64, String), Vec<(i64, i64)>> =
        std::collections::BTreeMap::new();
    let mut seen = std::collections::HashSet::new();
    for (message_id, loc) in located {
        if !seen.insert(message_id) {
            continue;
        }
        batches
            .entry((loc.account_id, loc.mailbox))
            .or_default()
            .push((message_id, loc.uid));
    }
    batches
        .into_iter()
        .map(|((account_id, mailbox), rows)| FolderBatch {
            account_id,
            mailbox,
            rows,
        })
        .collect()
}

/// Which failure a bulk run reports: the one it hit first.
///
/// why not the last, and why not abort: every batch runs, so one account
/// with a stale password cannot strand the rows of the others — and the
/// error worth showing is where the run first went wrong.
fn first_error(current: Result<(), AppError>, next: Result<(), AppError>) -> Result<(), AppError> {
    match current {
        Err(err) => Err(err),
        Ok(()) => next,
    }
}

/// `first_error` over a whole run of batch outcomes, in batch order — so a
/// selection that ran concurrently still reports the same error a
/// sequential loop would have.
fn first_error_of(outcomes: Vec<Result<(), AppError>>) -> Result<(), AppError> {
    outcomes.into_iter().fold(Ok(()), first_error)
}

/// Look every selected id up, then group them into per-folder batches.
///
/// `threads` mirrors what the row on screen stands for: in the conversation
/// views a row is a whole thread, so the action has to reach every member in
/// that folder, exactly as the single-row thread commands do. The flat
/// trash/junk/drafts views pass false — there a row is one message, and
/// expanding it would drag in siblings the user never selected.
async fn locate_batches(
    state: &AppState,
    message_ids: &[i64],
    threads: bool,
) -> Result<Vec<FolderBatch>, AppError> {
    let mut located = Vec::with_capacity(message_ids.len());
    for id in message_ids {
        let loc = storage::messages::location(&state.pool, *id).await?;
        if !threads {
            located.push((*id, loc));
            continue;
        }
        for (row_id, uid) in storage::messages::thread_rows_in_mailbox(&state.pool, *id).await? {
            located.push((
                row_id,
                storage::messages::MessageLocation {
                    account_id: loc.account_id,
                    mailbox: loc.mailbox.clone(),
                    uid,
                },
            ));
        }
    }
    Ok(group_by_folder(located))
}

/// Move a whole selection to a user-chosen folder — one server session per
/// account folder the selection touches.
///
/// why it keeps going after a failure: a selection can span accounts, and one
/// stale password must not strand the rows of every other account. The first
/// error is still returned, so the frontend can put the failed rows back.
pub async fn move_messages(
    app: &AppHandle,
    state: &AppState,
    message_ids: &[i64],
    mailbox: &str,
    threads: bool,
) -> Result<(), AppError> {
    let batches = locate_batches(state, message_ids, threads).await?;
    // why every account is checked before any move: the batches run
    // together below, so a folder found missing halfway could no longer
    // stop the earlier ones — refusing up front keeps "no folder" from
    // moving anything at all.
    for batch in &batches {
        if !storage::mailboxes::exists(&state.pool, batch.account_id, mailbox).await? {
            return Err(AppError::Imap(format!("no folder named {mailbox}")));
        }
    }
    // why join_all: each batch is one account folder on its own IMAP
    // connection, so run in sequence they only ever waited on each other's
    // round trips. Each batch still reconciles its own cache only after
    // its server confirms, exactly as before; join_all keeps the outcomes
    // in batch order, so the first error reported is the same one.
    let outcomes = futures::future::join_all(batches.iter().map(|batch| {
        move_rows_to_mailbox(
            app,
            state,
            batch.account_id,
            &batch.mailbox,
            &batch.rows,
            mailbox,
        )
    }))
    .await;
    first_error_of(outcomes)
}

/// The bulk counterpart of `move_to_special_folder`: each batch resolves the
/// destination against its own account, because "archive" and "trash" are
/// per-account folder names, not one shared string.
async fn move_batches_to_special_folder(
    app: &AppHandle,
    state: &AppState,
    message_ids: &[i64],
    threads: bool,
    roles: &[&str],
    label: &str,
) -> Result<(), AppError> {
    let batches = locate_batches(state, message_ids, threads).await?;
    // why the destinations are resolved first: same as move_messages — an
    // account without the folder must refuse the whole selection, not
    // whatever batches happened to finish before it was noticed.
    let mut dests = Vec::with_capacity(batches.len());
    for batch in &batches {
        dests.push(special_folder_dest(state, batch.account_id, roles, label).await?);
    }
    let outcomes = futures::future::join_all(batches.iter().zip(&dests).map(|(batch, dest)| {
        move_rows_to_mailbox(
            app,
            state,
            batch.account_id,
            &batch.mailbox,
            &batch.rows,
            dest,
        )
    }))
    .await;
    first_error_of(outcomes)
}

/// Trash a whole selection — each account's own Trash folder.
pub async fn trash_messages(
    app: &AppHandle,
    state: &AppState,
    message_ids: &[i64],
    threads: bool,
) -> Result<(), AppError> {
    move_batches_to_special_folder(app, state, message_ids, threads, &["trash"], "trash").await
}

/// Archive a whole selection — each account's own Archive (Gmail: All Mail).
pub async fn archive_messages(
    app: &AppHandle,
    state: &AppState,
    message_ids: &[i64],
    threads: bool,
) -> Result<(), AppError> {
    move_batches_to_special_folder(
        app,
        state,
        message_ids,
        threads,
        &["archive", "all"],
        "archive",
    )
    .await
}

/// Trash a whole conversation as shown in its folder — every thread member
/// sharing the anchor's mailbox. Flat views (trash/junk/drafts) keep using
/// the single-message commands; the frontend picks per row.
pub async fn trash_thread(
    app: &AppHandle,
    state: &AppState,
    message_id: i64,
) -> Result<(), AppError> {
    move_thread_to_special_folder(app, state, message_id, &["trash"], "trash").await
}

/// Archive a whole conversation as shown in its folder (Gmail: All Mail).
pub async fn archive_thread(
    app: &AppHandle,
    state: &AppState,
    message_id: i64,
) -> Result<(), AppError> {
    move_thread_to_special_folder(app, state, message_id, &["archive", "all"], "archive").await
}

/// Move a whole conversation (as shown in the anchor's folder) to a
/// user-chosen folder of its account.
pub async fn move_thread(
    app: &AppHandle,
    state: &AppState,
    message_id: i64,
    mailbox: &str,
) -> Result<(), AppError> {
    let loc = storage::messages::location(&state.pool, message_id).await?;
    if !storage::mailboxes::exists(&state.pool, loc.account_id, mailbox).await? {
        return Err(AppError::Imap(format!("no folder named {mailbox}")));
    }
    let rows = storage::messages::thread_rows_in_mailbox(&state.pool, message_id).await?;
    move_rows_to_mailbox(app, state, loc.account_id, &loc.mailbox, &rows, mailbox).await
}

/// Move a message to the account's folder for the first matching special-use
/// `role`, then drop it from the local cache. Unlike the read flag this waits
/// on the server — the row must not vanish from the list if the move failed.
async fn move_to_special_folder(
    app: &AppHandle,
    state: &AppState,
    message_id: i64,
    roles: &[&str],
    label: &str,
) -> Result<(), AppError> {
    let loc = storage::messages::location(&state.pool, message_id).await?;
    let dest = special_folder_dest(state, loc.account_id, roles, label).await?;
    let rows = vec![(message_id, loc.uid)];
    move_rows_to_mailbox(app, state, loc.account_id, &loc.mailbox, &rows, &dest).await
}

/// Like `move_to_special_folder`, but for the anchor's whole conversation
/// as shown in its folder — every thread member sharing the mailbox moves
/// in one server session.
async fn move_thread_to_special_folder(
    app: &AppHandle,
    state: &AppState,
    message_id: i64,
    roles: &[&str],
    label: &str,
) -> Result<(), AppError> {
    let loc = storage::messages::location(&state.pool, message_id).await?;
    let dest = special_folder_dest(state, loc.account_id, roles, label).await?;
    let rows = storage::messages::thread_rows_in_mailbox(&state.pool, message_id).await?;
    move_rows_to_mailbox(app, state, loc.account_id, &loc.mailbox, &rows, &dest).await
}

/// The account's folder name for the first matching special-use role.
async fn special_folder_dest(
    state: &AppState,
    account_id: i64,
    roles: &[&str],
    label: &str,
) -> Result<String, AppError> {
    for role in roles {
        if let Some(name) = storage::mailboxes::name_for_role(&state.pool, account_id, role).await?
        {
            return Ok(name);
        }
    }
    Err(AppError::Imap(format!("no {label} folder discovered yet")))
}

/// The one server-confirmed move path: select the source folder, UID MOVE
/// each `(row id, uid)` into `dest`, and drop cached rows only for confirmed
/// moves — a row must not vanish from the list if its move failed. A failure
/// mid-batch keeps the earlier moves (they already happened server-side) and
/// surfaces the error.
async fn move_rows_to_mailbox(
    app: &AppHandle,
    state: &AppState,
    account_id: i64,
    source: &str,
    rows: &[(i64, i64)],
    dest: &str,
) -> Result<(), AppError> {
    let _t = timing::start("cmd::move_rows_to_mailbox");
    // why: moving a message into the folder it already lives in is a no-op
    // (and some servers error on it) — just leave it be.
    if source == dest || rows.is_empty() {
        return Ok(());
    }

    let account = storage::accounts::get(&state.pool, account_id).await?;
    let credential = state.credential(&account).await?;
    let mut session = mail::imap::connect(
        &account.imap_host,
        account.imap_port,
        &account.username,
        &credential,
    )
    .await?;
    session
        .select(source)
        .await
        .map_err(|e| AppError::Imap(format!("select {source}: {e}")))?;
    let mut outcome = Ok(());
    let mut moved: Vec<i64> = Vec::new();
    for (row_id, uid) in rows {
        match mail::imap::move_message(&mut session, *uid, dest).await {
            Ok(()) => moved.push(*row_id),
            Err(err) => {
                outcome = Err(err);
                break;
            }
        }
    }
    let _ = session.logout().await;

    // why: only after the server confirms — each confirmed message left the
    // source folder, so its cached row (and images, via cascade) go too.
    let emit = !moved.is_empty();
    for row_id in moved {
        storage::messages::delete_by_id(&state.pool, row_id).await?;
    }
    if emit {
        app.emit("messages-changed", MessagesChanged::reload(account_id))?;
    }
    outcome
}

/// Connect, select the folder, flip the `\Seen` flag, log out.
async fn push_seen_flag(
    account: &Account,
    credential: &Credential,
    mailbox: &str,
    uid: i64,
    seen: bool,
) -> Result<(), AppError> {
    push_seen_flags(account, credential, mailbox, &[uid], seen).await
}

/// One connection, one SELECT, one STORE over the whole set.
async fn push_seen_flags(
    account: &Account,
    credential: &Credential,
    mailbox: &str,
    uids: &[i64],
    seen: bool,
) -> Result<(), AppError> {
    let _t = timing::start("cmd::push_seen_flags");
    let mut session = mail::imap::connect(
        &account.imap_host,
        account.imap_port,
        &account.username,
        credential,
    )
    .await?;
    session
        .select(mailbox)
        .await
        .map_err(|e| AppError::Imap(format!("select {mailbox}: {e}")))?;
    let result = mail::imap::set_seen_many(&mut session, uids, seen).await;
    let _ = session.logout().await;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn located(
        id: i64,
        account_id: i64,
        mailbox: &str,
        uid: i64,
    ) -> (i64, storage::messages::MessageLocation) {
        (
            id,
            storage::messages::MessageLocation {
                account_id,
                mailbox: mailbox.to_string(),
                uid,
            },
        )
    }

    fn imap_err(text: &str) -> Result<(), AppError> {
        Err(AppError::Imap(text.to_string()))
    }

    #[test]
    fn first_error_of_reports_the_earliest_failure_in_batch_order() {
        assert!(first_error_of(vec![]).is_ok());
        assert!(first_error_of(vec![Ok(()), Ok(())]).is_ok());
        assert_eq!(
            first_error_of(vec![Ok(()), imap_err("second"), imap_err("third")])
                .unwrap_err()
                .to_string(),
            imap_err("second").unwrap_err().to_string()
        );
    }

    #[test]
    fn a_bulk_run_reports_the_first_failure_it_hit() {
        assert!(first_error(Ok(()), Ok(())).is_ok());
        assert_eq!(
            first_error(Ok(()), imap_err("second"))
                .unwrap_err()
                .to_string(),
            AppError::Imap("second".to_string()).to_string(),
        );
        // Later batches still ran; their errors must not mask the first.
        assert_eq!(
            first_error(imap_err("first"), imap_err("second"))
                .unwrap_err()
                .to_string(),
            AppError::Imap("first".to_string()).to_string(),
        );
        assert_eq!(
            first_error(imap_err("first"), Ok(()))
                .unwrap_err()
                .to_string(),
            AppError::Imap("first".to_string()).to_string(),
        );
    }

    #[test]
    fn grouping_an_empty_selection_yields_no_work() {
        assert_eq!(group_by_folder(Vec::new()), Vec::new());
    }

    #[test]
    fn rows_of_one_folder_share_a_single_batch() {
        let batches = group_by_folder(vec![
            located(1, 7, "INBOX", 100),
            located(2, 7, "INBOX", 101),
        ]);

        assert_eq!(
            batches,
            vec![FolderBatch {
                account_id: 7,
                mailbox: "INBOX".to_string(),
                rows: vec![(1, 100), (2, 101)],
            }]
        );
    }

    // Expanding conversations can name the same row twice — two selected
    // rows of one thread, say. Moving a uid twice would fail the second time.
    #[test]
    fn a_row_named_twice_is_moved_once() {
        let batches = group_by_folder(vec![
            located(1, 7, "INBOX", 100),
            located(2, 7, "INBOX", 101),
            located(1, 7, "INBOX", 100),
        ]);

        assert_eq!(
            batches,
            vec![FolderBatch {
                account_id: 7,
                mailbox: "INBOX".to_string(),
                rows: vec![(1, 100), (2, 101)],
            }]
        );
    }

    #[test]
    fn folders_and_accounts_each_get_their_own_batch() {
        // A unified-inbox selection: two accounts, and one of them spans two
        // folders. Every batch is one IMAP session.
        let batches = group_by_folder(vec![
            located(1, 7, "INBOX", 100),
            located(2, 9, "INBOX", 200),
            located(3, 7, "Archive", 300),
            located(4, 7, "INBOX", 101),
        ]);

        assert_eq!(
            batches,
            vec![
                FolderBatch {
                    account_id: 7,
                    mailbox: "Archive".to_string(),
                    rows: vec![(3, 300)],
                },
                FolderBatch {
                    account_id: 7,
                    mailbox: "INBOX".to_string(),
                    rows: vec![(1, 100), (4, 101)],
                },
                FolderBatch {
                    account_id: 9,
                    mailbox: "INBOX".to_string(),
                    rows: vec![(2, 200)],
                },
            ]
        );
    }
}
