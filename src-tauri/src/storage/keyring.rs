//! 系统钥匙串存取。
//!
//! 缺陷 #1：旧版把密码和私钥口令以明文存进浏览器 `localStorage`，
//! 任何能读取用户目录的进程都能拿到。这里改用操作系统钥匙串：
//! macOS Keychain / Windows Credential Manager / Linux Secret Service。
//!
//! 钥匙串不可用时（无 GUI 会话、Linux 缺 libsecret 等）**不降级为明文文件**，
//! 而是返回 `KeyringUnavailable`，由界面明确告知用户「本次连接有效但不会被保存」。

use crate::error::{AppError, AppResult};

/// 钥匙串条目的 service 名。
const SERVICE: &str = "ssh7";

/// 密码在钥匙串中的账号名。
pub fn password_account(host_id: &str) -> String {
    format!("{host_id}/password")
}

/// 私钥口令在钥匙串中的账号名。
pub fn passphrase_account(host_id: &str) -> String {
    format!("{host_id}/passphrase")
}

fn entry(account: &str) -> AppResult<keyring::Entry> {
    keyring::Entry::new(SERVICE, account)
        .map_err(|e| AppError::KeyringUnavailable(format!("无法访问系统钥匙串: {e}")))
}

/// 写入一条秘密。
pub fn set(account: &str, secret: &str) -> AppResult<()> {
    entry(account)?
        .set_password(secret)
        .map_err(|e| AppError::KeyringUnavailable(format!("写入钥匙串失败: {e}")))
}

/// 读取一条秘密。钥匙串里没有该条目时返回 `Ok(None)`。
pub fn get(account: &str) -> AppResult<Option<String>> {
    match entry(account)?.get_password() {
        Ok(v) => Ok(Some(v)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(AppError::KeyringUnavailable(format!("读取钥匙串失败: {e}"))),
    }
}

/// 删除一条秘密。条目不存在时视为成功。
pub fn delete(account: &str) -> AppResult<()> {
    match entry(account)?.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(AppError::KeyringUnavailable(format!(
            "删除钥匙串条目失败: {e}"
        ))),
    }
}

/// 探测钥匙串是否可用（用于启动时提示用户）。
pub fn probe() -> bool {
    let probe_account = "__probe__";
    if set(probe_account, "1").is_ok() {
        let _ = delete(probe_account);
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_names_are_namespaced_by_host() {
        assert_eq!(password_account("abc"), "abc/password");
        assert_eq!(passphrase_account("abc"), "abc/passphrase");
    }

    #[test]
    fn missing_entry_reads_as_none() {
        // 没有对应条目时应返回 None 而不是错误。
        let acct = "ssh7-unit-test-nonexistent";
        match get(acct) {
            Ok(v) => assert!(v.is_none(), "不存在的条目应返回 None，实际 {:?}", v),
            // 钥匙串不可用时返回 KeyringUnavailable，这也是合法结果。
            Err(AppError::KeyringUnavailable(_)) => {}
            Err(other) => panic!("意外错误: {other}"),
        }
    }
}
