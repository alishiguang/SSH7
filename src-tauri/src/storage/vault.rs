//! 本地加密凭据库。
//!
//! 为什么需要它：
//!
//! macOS 的钥匙串按**程序身份**授权，而未签名（ad-hoc 签名）的应用每升级
//! 一次都会变成新的身份 —— 于是每次都要弹窗索要「登录」钥匙串密码。
//! 路径信任（`security -T`）、放行任意应用（`-A`）都救不了：
//! 钥匙串只认代码签名，ad-hoc 签名的 designated requirement 绑的是
//! 二进制哈希，重编译即失效。
//!
//! 所以这里提供一个**不依赖系统钥匙串**的替代后端：
//!
//! - `vault.key`：32 字节随机主密钥，权限 0600
//! - `vault.bin`：AES-256-GCM 密文（ring），权限 0600
//!
//! 威胁模型要说清楚：它与「钥匙串 + 放行任意应用」相当 —— 能以当前用户
//! 身份运行的进程都能解开。它真正防住的是「明文躺在磁盘上」「配置文件被
//! 同步 / 拷贝出去」这两种情况。

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM, NONCE_LEN};
use ring::rand::{SecureRandom, SystemRandom};

use crate::error::{AppError, AppResult};

const KEY_LEN: usize = 32;
const KEY_FILE: &str = "vault.key";
const DATA_FILE: &str = "vault.bin";

/// 加密附加数据：绑定用途，避免密文被挪到别的位置使用。
const AAD: &[u8] = b"ssh7-credential-vault-v1";

/// 本地加密凭据库。
pub struct Vault {
    dir: PathBuf,
}

impl Vault {
    /// 绑定到应用配置目录。
    pub fn new(dir: &Path) -> Self {
        Self {
            dir: dir.to_path_buf(),
        }
    }

    fn key_path(&self) -> PathBuf {
        self.dir.join(KEY_FILE)
    }

    fn data_path(&self) -> PathBuf {
        self.dir.join(DATA_FILE)
    }

    /// 读取主密钥；不存在时生成一个新的（并写入 0600 权限的文件）。
    fn master_key(&self) -> AppResult<[u8; KEY_LEN]> {
        let path = self.key_path();
        if path.exists() {
            let raw = fs::read(&path)?;
            if raw.len() != KEY_LEN {
                return Err(AppError::Internal(
                    "凭据库主密钥长度异常，已损坏；请删除凭据库后重新保存密码".into(),
                ));
            }
            let mut key = [0u8; KEY_LEN];
            key.copy_from_slice(&raw);
            return Ok(key);
        }

        let mut key = [0u8; KEY_LEN];
        SystemRandom::new()
            .fill(&mut key)
            .map_err(|_| AppError::Internal("无法生成凭据库主密钥".into()))?;
        write_private(&path, &key)?;
        Ok(key)
    }

    /// 解出全部条目。文件不存在时返回空表。
    fn load_map(&self) -> AppResult<BTreeMap<String, String>> {
        let path = self.data_path();
        if !path.exists() {
            return Ok(BTreeMap::new());
        }
        let raw = fs::read(&path)?;
        if raw.len() <= NONCE_LEN {
            return Err(AppError::Internal("凭据库文件已损坏".into()));
        }

        let key = LessSafeKey::new(
            UnboundKey::new(&AES_256_GCM, &self.master_key()?)
                .map_err(|e| AppError::Internal(format!("凭据库密钥无效: {e}")))?,
        );

        let (nonce_bytes, ciphertext) = raw.split_at(NONCE_LEN);
        let mut nonce = [0u8; NONCE_LEN];
        nonce.copy_from_slice(nonce_bytes);
        let mut in_out = ciphertext.to_vec();

        let plaintext = key
            .open_in_place(
                Nonce::assume_unique_for_key(nonce),
                Aad::from(AAD),
                &mut in_out,
            )
            .map_err(|_| AppError::Internal("凭据库解密失败，主密钥可能已被替换".into()))?;

        serde_json::from_slice(plaintext)
            .map_err(|e| AppError::Internal(format!("凭据库内容解析失败: {e}")))
    }

    /// 加密并写回全部条目。
    fn save_map(&self, map: &BTreeMap<String, String>) -> AppResult<()> {
        let plaintext = serde_json::to_vec(map)
            .map_err(|e| AppError::Internal(format!("凭据库序列化失败: {e}")))?;

        let key = LessSafeKey::new(
            UnboundKey::new(&AES_256_GCM, &self.master_key()?)
                .map_err(|e| AppError::Internal(format!("凭据库密钥无效: {e}")))?,
        );

        let mut nonce_bytes = [0u8; NONCE_LEN];
        SystemRandom::new()
            .fill(&mut nonce_bytes)
            .map_err(|_| AppError::Internal("无法生成随机数".into()))?;

        let mut in_out = plaintext;
        key.seal_in_place_append_tag(
            Nonce::assume_unique_for_key(nonce_bytes),
            Aad::from(AAD),
            &mut in_out,
        )
        .map_err(|_| AppError::Internal("凭据库加密失败".into()))?;

        let mut blob = nonce_bytes.to_vec();
        blob.extend_from_slice(&in_out);
        write_private(&self.data_path(), &blob)
    }

    /// 写入一条秘密。
    pub fn set(&self, account: &str, secret: &str) -> AppResult<()> {
        let mut map = self.load_map()?;
        map.insert(account.to_string(), secret.to_string());
        self.save_map(&map)
    }

    /// 读取一条秘密。
    pub fn get(&self, account: &str) -> AppResult<Option<String>> {
        Ok(self.load_map()?.get(account).cloned())
    }

    /// 删除一条秘密。
    pub fn delete(&self, account: &str) -> AppResult<()> {
        let mut map = self.load_map()?;
        if map.remove(account).is_some() {
            self.save_map(&map)?;
        }
        Ok(())
    }

    /// 库里有没有东西。
    pub fn is_empty(&self) -> bool {
        self.load_map().map(|m| m.is_empty()).unwrap_or(true)
    }
}

/// 以 0600 权限写文件（Unix）；Windows 上忽略。
fn write_private(path: &Path, data: &[u8]) -> AppResult<()> {
    use std::io::Write;

    let mut opts = fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts
        .open(path)
        .map_err(|e| AppError::Io(format!("无法写入 {}: {e}", path.display())))?;
    f.write_all(data)
        .map_err(|e| AppError::Io(format!("无法写入 {}: {e}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("ssh7-vault-test-{tag}-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn stores_and_reads_back() {
        let dir = temp_dir("basic");
        let vault = Vault::new(&dir);

        assert!(vault.get("a/password").unwrap().is_none());
        vault.set("a/password", "hunter2").unwrap();
        assert_eq!(vault.get("a/password").unwrap().as_deref(), Some("hunter2"));

        vault.set("b/passphrase", "s3cret").unwrap();
        assert_eq!(
            vault.get("b/passphrase").unwrap().as_deref(),
            Some("s3cret")
        );

        vault.delete("a/password").unwrap();
        assert!(vault.get("a/password").unwrap().is_none());
        // 删掉一条不影响另一条。
        assert_eq!(
            vault.get("b/passphrase").unwrap().as_deref(),
            Some("s3cret")
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn ciphertext_does_not_contain_the_secret() {
        let dir = temp_dir("cipher");
        let vault = Vault::new(&dir);
        vault.set("a/password", "plaintext-marker").unwrap();

        let raw = std::fs::read(dir.join(DATA_FILE)).unwrap();
        let text = String::from_utf8_lossy(&raw);
        assert!(!text.contains("plaintext-marker"), "密文里不该出现明文");
        // 账号名同样不该明文可见。
        assert!(!text.contains("a/password"), "账号名不该明文可见");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn key_file_is_private() {
        let dir = temp_dir("perm");
        let vault = Vault::new(&dir);
        vault.set("a/password", "x").unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.join(KEY_FILE))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600, "主密钥必须是 0600");
            let mode = std::fs::metadata(dir.join(DATA_FILE))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600, "密文文件必须是 0600");
        }

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn wrong_key_cannot_read() {
        let dir = temp_dir("wrongkey");
        let vault = Vault::new(&dir);
        vault.set("a/password", "hunter2").unwrap();

        // 换一把主密钥，原密文解不开。
        let mut other = [0u8; KEY_LEN];
        other[0] = 0xAB;
        write_private(&dir.join(KEY_FILE), &other).unwrap();

        let err = vault.get("a/password").unwrap_err();
        assert_eq!(err.code(), crate::error::ErrorCode::Internal);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_key_file_is_regenerated_but_old_data_is_unreadable() {
        let dir = temp_dir("regen");
        let vault = Vault::new(&dir);
        vault.set("a/password", "hunter2").unwrap();

        std::fs::remove_file(dir.join(KEY_FILE)).unwrap();
        assert!(
            vault.get("a/password").is_err(),
            "主密钥丢失后不应能读出内容"
        );

        std::fs::remove_dir_all(&dir).ok();
    }
}
