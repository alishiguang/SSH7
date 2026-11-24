//! 认证凭据的解析。
//!
//! 私钥解析统一在 Rust 侧完成，前端永远不接触私钥内容。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use russh::keys::PrivateKey;

use crate::error::{AppError, AppResult};

/// 认证凭据。
#[derive(Clone)]
pub enum Credential {
    Password(String),
    PrivateKey { key: Arc<PrivateKey> },
}

impl Credential {
    /// 凭据的种类，用于日志与错误提示。
    pub fn kind(&self) -> &'static str {
        match self {
            Credential::Password(_) => "密码",
            Credential::PrivateKey { .. } => "私钥",
        }
    }
}

/// 把 `~` 展开为用户主目录。
pub fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    if path == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    }
    PathBuf::from(path)
}

/// 读取并解析私钥。
///
/// 支持：
/// - OpenSSH 格式（`-----BEGIN OPENSSH PRIVATE KEY-----`），含加密版本；
/// - 通过 `ssh-keygen -i -m PEM` 转换的传统 PKCS#1 / PKCS#8 PEM。
///
/// 失败时返回结构化错误，**不会**把私钥内容写进错误消息。
pub fn load_private_key(path: &str, passphrase: Option<&str>) -> AppResult<Arc<PrivateKey>> {
    let expanded = expand_tilde(path);

    let raw = std::fs::read(&expanded)
        .map_err(|e| AppError::KeyInvalid(format!("无法读取私钥 {}: {}", expanded.display(), e)))?;

    let mut key = match PrivateKey::from_openssh(&raw) {
        Ok(k) => k,
        Err(open_err) => match parse_legacy_pem(&raw) {
            Ok(k) => k,
            Err(_) => {
                return Err(AppError::KeyInvalid(format!(
                    "私钥格式无法解析（{}）：请确认是 OpenSSH 格式，或用 ssh-keygen 转换",
                    expanded.display()
                ))
                .with_detail(open_err.to_string()))
            }
        },
    };

    if key.is_encrypted() {
        let pass = passphrase
            .filter(|p| !p.is_empty())
            .ok_or_else(|| AppError::KeyInvalid("私钥已加密，请提供私钥口令".to_string()))?;
        key = key
            .decrypt(pass.as_bytes())
            .map_err(|_| AppError::KeyInvalid("私钥口令错误".to_string()))?;
    }

    Ok(Arc::new(key))
}

/// 尝试解析传统 PEM 私钥（PKCS#1 `RSA PRIVATE KEY` / PKCS#8 `PRIVATE KEY`）。
///
/// ssh-key 只支持 OpenSSH 格式，这里借助系统自带的 `ssh-keygen` 做一次格式转换。
/// `ssh-keygen` 不存在或转换失败时返回错误，调用方会给出明确提示。
fn parse_legacy_pem(raw: &[u8]) -> Result<PrivateKey, String> {
    let text = String::from_utf8_lossy(raw);
    let looks_legacy = text.contains("-----BEGIN RSA PRIVATE KEY-----")
        || text.contains("-----BEGIN PRIVATE KEY-----")
        || text.contains("-----BEGIN EC PRIVATE KEY-----");
    if !looks_legacy {
        return Err("不是传统 PEM 格式".into());
    }

    let dir = std::env::temp_dir();
    let name = format!("ssh7-key-{}", uuid::Uuid::now_v7());
    let path = dir.join(name);

    // 写入临时文件，权限设为 0600。
    {
        use std::io::Write;
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&path).map_err(|e| e.to_string())?;
        f.write_all(raw).map_err(|e| e.to_string())?;
    }

    let out = std::process::Command::new("ssh-keygen")
        .args(["-i", "-m", "PEM", "-f"])
        .arg(&path)
        .output();

    let result = match out {
        Ok(o) if o.status.success() => {
            let converted = std::fs::read(&path).map_err(|e| e.to_string())?;
            PrivateKey::from_openssh(&converted).map_err(|e| e.to_string())
        }
        Ok(o) => Err(String::from_utf8_lossy(&o.stderr).trim().to_string()),
        Err(e) => Err(format!("ssh-keygen 不可用: {e}")),
    };

    let _ = std::fs::remove_file(&path);
    result
}

impl AppError {
    /// 给错误附加更详细的技术说明（前端展示用 message，展示详情用）。
    fn with_detail(self, detail: String) -> Self {
        match self {
            AppError::KeyInvalid(msg) => AppError::KeyInvalid(format!("{msg}（{detail}）")),
            other => other,
        }
    }
}

/// 校验本地文件是否存在且可读，用于上传前的快速失败。
pub fn ensure_readable(path: &Path) -> AppResult<u64> {
    let meta = std::fs::metadata(path)?;
    if !meta.is_file() {
        return Err(AppError::InvalidInput(format!(
            "{} 不是普通文件",
            path.display()
        )));
    }
    Ok(meta.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_home_directory() {
        let expanded = expand_tilde("~/.ssh/id_ed25519");
        assert!(expanded.to_string_lossy().contains(".ssh/id_ed25519"));
        assert!(!expanded.to_string_lossy().starts_with('~'));
    }

    #[test]
    fn leaves_absolute_paths_untouched() {
        assert_eq!(expand_tilde("/etc/hosts"), PathBuf::from("/etc/hosts"));
        assert_eq!(expand_tilde("relative/key"), PathBuf::from("relative/key"));
    }

    #[test]
    fn missing_key_file_is_reported_cleanly() {
        let err = load_private_key("/definitely/not/here/key", None).unwrap_err();
        assert_eq!(err.code(), crate::error::ErrorCode::KeyInvalid);
        // 错误消息里不能出现私钥内容
        assert!(!err.to_string().contains("PRIVATE KEY"));
    }

    #[test]
    fn unparsable_key_is_reported_cleanly() {
        let dir = std::env::temp_dir().join(format!("ssh7-badkey-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("key");
        std::fs::write(&p, b"this is not a key").unwrap();
        let err = load_private_key(p.to_str().unwrap(), None).unwrap_err();
        assert_eq!(err.code(), crate::error::ErrorCode::KeyInvalid);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
