//! 凭据存储：**只有本地加密文件**一种后端。
//!
//! 早期版本有两种：系统钥匙串（macOS Keychain / Windows 凭据管理器 /
//! Linux Secret Service）和应用目录内的 AES-256-GCM 加密文件，
//! 可以在设置面板里切换。现在**只剩后者**，而且是**唯一**的形态：
//!
//! - macOS 上未签名的应用每升级一次就是新身份，系统钥匙串会反复弹窗索要授权；
//! - Linux 上没装 Secret Service 时钥匙串压根不可用；
//! - 钥匙串是**平台专属**的：Windows 走的是凭据管理器，Linux 走 Secret Service，
//!   三套 API、三套失败模式 —— 跨平台产品不该背着这种差异；
//! - 「极简」是产品主张 —— 不该让用户在两种存放方式之间做选择。
//!
//! 所以现在**完全不依赖系统钥匙串**：所有平台、所有版本，密码都进同一个
//! 加密文件（`vault.bin`），行为完全一致。

use std::path::{Path, PathBuf};

use crate::error::AppResult;
use crate::storage::vault::Vault;

/// 旧版本留下的「凭据后端偏好」文件。现在只有一种后端了，它没有任何意义 ——
/// 启动时顺手删掉，免得留在磁盘上让人以为还能切。
pub const LEGACY_PREF_FILE: &str = "credential-store.json";

/// 密码条目的账号名。
pub fn password_account(host_id: &str) -> String {
    format!("{host_id}/password")
}

/// 私钥口令条目的账号名。
pub fn passphrase_account(host_id: &str) -> String {
    format!("{host_id}/passphrase")
}

/// 凭据存放位置。
pub struct SecretStore {
    dir: PathBuf,
}

impl SecretStore {
    pub fn new(dir: &Path) -> Self {
        Self {
            dir: dir.to_path_buf(),
        }
    }

    fn vault(&self) -> Vault {
        Vault::new(&self.dir)
    }

    /// 写入一条秘密。
    pub fn set(&self, account: &str, secret: &str) -> AppResult<()> {
        self.vault().set(account, secret)
    }

    /// 读取一条秘密。没有就是没有 —— 不再回退去问系统钥匙串。
    pub fn get(&self, account: &str) -> AppResult<Option<String>> {
        self.vault().get(account)
    }

    /// 删除一条秘密。
    pub fn delete(&self, account: &str) -> AppResult<()> {
        self.vault().delete(account)
    }

    /// 删掉旧版本留下的后端偏好文件。
    ///
    /// 那份偏好（`{"backend": "keyring"}` 之类）现在完全用不上了：
    /// 后端只有一种。留着它只会让人以为还能切。
    pub fn remove_legacy_prefs(&self) {
        let _ = std::fs::remove_file(self.dir.join(LEGACY_PREF_FILE));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("ssh7-secrets-test-{tag}-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn account_names_are_namespaced_by_host() {
        assert_eq!(password_account("abc"), "abc/password");
        assert_eq!(passphrase_account("abc"), "abc/passphrase");
    }

    #[test]
    fn vault_round_trips_secrets() {
        let dir = temp_dir("vault");
        let store = SecretStore::new(&dir);

        store.set("host/password", "hunter2").unwrap();
        assert_eq!(
            store.get("host/password").unwrap().as_deref(),
            Some("hunter2")
        );

        store.delete("host/password").unwrap();
        assert!(store.get("host/password").unwrap().is_none());

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 没有秘密时读取应返回 `None`，**不是**报错。
    #[test]
    fn missing_secret_reads_as_none() {
        let dir = temp_dir("missing");
        let store = SecretStore::new(&dir);

        assert_eq!(store.get("host/password").unwrap(), None);

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 旧的后端偏好文件会被清掉。
    #[test]
    fn legacy_preference_file_is_cleaned_up() {
        let dir = temp_dir("legacy");
        let pref = dir.join(LEGACY_PREF_FILE);
        std::fs::write(&pref, b"{\"backend\": \"keyring\"}").unwrap();

        SecretStore::new(&dir).remove_legacy_prefs();
        assert!(!pref.exists(), "旧偏好文件应该被删掉");

        std::fs::remove_dir_all(&dir).ok();
    }
}
