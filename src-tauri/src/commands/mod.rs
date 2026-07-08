use crate::mock;
use crate::models::{Account, MessageHeader};

// why: commands return Result even though mocks can't fail — the signature is
// the frontend contract, and it stays stable when Phase 1 swaps mocks for
// real IO that *can* fail.

#[tauri::command]
pub fn list_accounts() -> Result<Vec<Account>, String> {
    Ok(mock::accounts())
}

#[tauri::command]
pub fn list_messages(account_id: Option<i64>) -> Result<Vec<MessageHeader>, String> {
    Ok(mock::messages(account_id))
}
