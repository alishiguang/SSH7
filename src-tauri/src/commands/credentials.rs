//! 凭据存储后端的查询与切换。

use tauri::State;

use crate::commands::AppState;
use crate::error::{AppError, AppResult};
use crate::storage::keyring;
use crate::storage::secrets::{default_backend, SecretBackend, SecretStore};

/// 凭据存储信息（前端展示用）。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialStoreInfo {
    /// 当前后端：`keyring` | `vault`。
    pub backend: String,
    /// 当前后端的展示名。
    pub backend_label: String,
    /// 系统钥匙串是否可用（Linux 上可能没有 Secret Service）。
    pub keyring_available: bool,
    /// 当前后端是不是该平台的默认值。
    pub is_default: bool,
    /// 该平台的默认后端。
    pub default_backend: String,
}

fn store_info(state: &AppState) -> CredentialStoreInfo {
    let backend = state.secrets.backend();
    CredentialStoreInfo {
        backend: backend.as_str().to_string(),
        backend_label: backend.label().to_string(),
        keyring_available: SecretStore::keyring_available(),
        is_default: backend == default_backend(),
        default_backend: default_backend().as_str().to_string(),
    }
}

/// 查询当前凭据存储方式。
#[tauri::command]
pub fn credential_store_info(state: State<'_, AppState>) -> AppResult<CredentialStoreInfo> {
    Ok(store_info(&state))
}

/// 切换凭据存储方式。
///
/// 已保存的秘密会一起搬到新后端：从钥匙串搬到本地加密文件时，
/// macOS 会弹一次钥匙串授权（之后就不再弹了）。
#[tauri::command]
pub fn credential_store_set(
    state: State<'_, AppState>,
    backend: String,
) -> AppResult<CredentialStoreInfo> {
    let target = match backend.as_str() {
        "keyring" => SecretBackend::Keyring,
        "vault" => SecretBackend::Vault,
        other => {
            return Err(AppError::InvalidInput(format!(
                "未知的凭据存储方式: {other}"
            )))
        }
    };

    // 收集需要迁移的条目。
    let hosts = state.hosts.load()?;
    let mut accounts = Vec::new();
    for h in hosts {
        if h.has_password {
            accounts.push(keyring::password_account(&h.id));
        }
        if h.has_passphrase {
            accounts.push(keyring::passphrase_account(&h.id));
        }
    }

    state.secrets.set_backend(target, &accounts)?;
    Ok(store_info(&state))
}
