use crate::error::AppError;

// why: the Keychain "service" is the bundle identifier, so Flit's entries
// group predictably under one name in Keychain Access.app.
const SERVICE: &str = "sk.vocalio.flit";

/// Keychain user name for an account row id.
///
/// why: keyed by id, not email — emails are user-editable and two accounts
/// may share one address; the row id is stable and unique.
fn keychain_user(account_id: i64) -> String {
    format!("account-{account_id}")
}

/// True when the error just means "no such credential stored".
fn is_missing_credential(err: &keyring::Error) -> bool {
    matches!(err, keyring::Error::NoEntry)
}

// why: the keyring API is synchronous — spawn_blocking moves each call off
// the async executor threads, so a slow Keychain access (or a user prompt)
// can't stall unrelated tasks. The outer `?` maps a panicked/cancelled task
// (JoinError); the inner Result is the keychain outcome.

pub async fn set_password(account_id: i64, password: String) -> Result<(), AppError> {
    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        let entry = keyring::Entry::new(SERVICE, &keychain_user(account_id))?;
        entry.set_password(&password)?;
        Ok(())
    })
    .await?
}

pub async fn get_password(account_id: i64) -> Result<String, AppError> {
    tokio::task::spawn_blocking(move || -> Result<String, AppError> {
        let entry = keyring::Entry::new(SERVICE, &keychain_user(account_id))?;
        Ok(entry.get_password()?)
    })
    .await?
}

/// Delete the stored password. Idempotent — a missing credential is already
/// the desired end state.
pub async fn delete_password(account_id: i64) -> Result<(), AppError> {
    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        let entry = keyring::Entry::new(SERVICE, &keychain_user(account_id))?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(err) if is_missing_credential(&err) => Ok(()),
            Err(err) => Err(err.into()),
        }
    })
    .await?
}

// why: only the pure parts are unit-tested. The real Keychain can't run in CI
// (it would prompt and mutate the developer's keychain), and keyring's v1
// facade pins the platform store on first use, so its mock store can't be
// injected here. The thin wrappers above get exercised manually via the app.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keychain_user_embeds_account_id() {
        assert_eq!(keychain_user(42), "account-42");
    }

    #[test]
    fn no_entry_counts_as_missing_credential() {
        assert!(is_missing_credential(&keyring::Error::NoEntry));
    }

    #[test]
    fn other_errors_are_not_missing_credential() {
        let err = keyring::Error::BadStoreFormat("broken".to_string());
        assert!(!is_missing_credential(&err));
    }
}
