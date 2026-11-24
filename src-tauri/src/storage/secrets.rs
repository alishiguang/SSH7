//! 凭据存储后端。
//!
//! 两种后端：
//!
//! - [`SecretBackend::Keyring`]：系统钥匙串 / 凭据管理器 / Secret Service。
//!   Windows 与 Linux 上没有 macOS 那个「每次升级都要重新授权」的问题，
//!   因此在这两个平台是默认选项。
//! - [`SecretBackend::Vault`]：应用目录内的 AES-256-GCM 加密文件，
//!   不依赖系统钥匙串，也不会弹窗。macOS 上未签名的应用默认用它。
//!
//! 偏好本身不是秘密，明文存在 `credential-store.json` 里。

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::error::AppResult;
use crate::storage::keyring;
use crate::storage::vault::Vault;

/// 偏好文件名。
const PREF_FILE: &str = "credential-store.json";

/// 凭据存放位置。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SecretBackend {
    /// 系统钥匙串（macOS Keychain / Windows 凭据管理器 / Secret Service）。
    Keyring,
    /// 应用目录内的加密文件。
    Vault,
}

impl SecretBackend {
    pub fn as_str(self) -> &'static str {
        match self {
            SecretBackend::Keyring => "keyring",
            SecretBackend::Vault => "vault",
        }
    }

    /// 供界面展示的名称。
    pub fn label(self) -> &'static str {
        match self {
            SecretBackend::Keyring => "系统钥匙串",
            SecretBackend::Vault => "本地加密文件",
        }
    }
}

/// 偏好文件的内容。
#[derive(Debug, Serialize, Deserialize)]
struct PrefFile {
    backend: SecretBackend,
}

/// 该平台的默认后端（不探测钥匙串）。
///
/// macOS 上一律是本地加密文件：未签名的应用每次升级都会失去钥匙串授权，
/// 反复弹窗索要「登录」钥匙串密码。
pub fn platform_default_backend() -> SecretBackend {
    if cfg!(target_os = "macos") {
        SecretBackend::Vault
    } else {
        SecretBackend::Keyring
    }
}

/// 实际默认后端。
///
/// 非 macOS 平台上，钥匙串真的可用才用它 —— Linux 上没装 Secret Service
/// 时别把用户卡死在一个用不了的后端上：那样连「保存一台带密码的主机」
/// 都会直接失败。钥匙串不可用时退回本地加密文件。
pub fn default_backend() -> SecretBackend {
    match platform_default_backend() {
        SecretBackend::Vault => SecretBackend::Vault,
        SecretBackend::Keyring => {
            if keyring::probe() {
                SecretBackend::Keyring
            } else {
                SecretBackend::Vault
            }
        }
    }
}

/// 凭据存储门面：对外只暴露 set / get / delete，内部按偏好分派。
pub struct SecretStore {
    dir: PathBuf,
    /// 该平台的默认后端，算一次就够（探测钥匙串有副作用）。
    default: OnceLock<SecretBackend>,
}

impl SecretStore {
    pub fn new(dir: &Path) -> Self {
        Self {
            dir: dir.to_path_buf(),
            default: OnceLock::new(),
        }
    }

    /// 实际默认后端（带缓存）。
    pub fn default_backend(&self) -> SecretBackend {
        *self.default.get_or_init(default_backend)
    }

    fn pref_path(&self) -> PathBuf {
        self.dir.join(PREF_FILE)
    }

    /// 当前后端。偏好文件缺失或损坏时回落到默认后端。
    pub fn backend(&self) -> SecretBackend {
        let Ok(raw) = std::fs::read(self.pref_path()) else {
            return self.default_backend();
        };
        serde_json::from_slice::<PrefFile>(&raw)
            .map(|p| p.backend)
            .unwrap_or_else(|_| self.default_backend())
    }

    /// 切换后端，并把已有秘密搬过去。
    pub fn set_backend(&self, backend: SecretBackend, accounts: &[String]) -> AppResult<()> {
        let current = self.backend();
        if current != backend {
            self.migrate(current, backend, accounts)?;
        }
        self.write_backend(backend)
    }

    fn write_backend(&self, backend: SecretBackend) -> AppResult<()> {
        let body = serde_json::to_vec_pretty(&PrefFile { backend }).map_err(|e| {
            crate::error::AppError::Internal(format!("凭据存储偏好序列化失败: {e}"))
        })?;
        std::fs::create_dir_all(&self.dir)?;
        std::fs::write(self.pref_path(), body)?;
        Ok(())
    }

    /// 把 `from` 后端里的秘密复制到 `to` 后端。
    fn migrate(
        &self,
        from: SecretBackend,
        to: SecretBackend,
        accounts: &[String],
    ) -> AppResult<()> {
        if from == to {
            return Ok(());
        }
        for account in accounts {
            // 源后端读不出来时（钥匙串不可用、没授权）跳过这一条 ——
            // 读不到就没有东西可搬，不该让整个切换失败。
            // 目标后端写不进去则要报错：那说明用户选了一个用不了的后端。
            let Some(secret) = self.read_optional_from(from, account) else {
                continue;
            };
            self.write_to(to, account, &secret)?;
            let _ = self.delete_from(from, account);
        }
        Ok(())
    }

    fn read_from(&self, backend: SecretBackend, account: &str) -> AppResult<Option<String>> {
        match backend {
            SecretBackend::Keyring => keyring::get(account),
            SecretBackend::Vault => Vault::new(&self.dir).get(account),
        }
    }

    /// 读一个**可能读不到**的后端。
    ///
    /// 钥匙串在很多场合是不可用的：Linux 上没装 Secret Service、
    /// macOS 上条目不存在、或者用户没点「允许」。
    /// 这些情况下必须按「没有这条秘密」处理 —— 用户明明选了当前后端，
    /// 不该因为另一个后端不可用就把整个读取搞失败。
    fn read_optional_from(&self, backend: SecretBackend, account: &str) -> Option<String> {
        self.read_from(backend, account).ok().flatten()
    }

    fn write_to(&self, backend: SecretBackend, account: &str, secret: &str) -> AppResult<()> {
        match backend {
            SecretBackend::Keyring => keyring::set(account, secret),
            SecretBackend::Vault => Vault::new(&self.dir).set(account, secret),
        }
    }

    fn delete_from(&self, backend: SecretBackend, account: &str) -> AppResult<()> {
        match backend {
            SecretBackend::Keyring => keyring::delete(account),
            SecretBackend::Vault => Vault::new(&self.dir).delete(account),
        }
    }

    /// 确保偏好文件存在：首次运行时把当前后端写进去。
    ///
    /// 否则「当前用的哪个后端」只能靠「文件缺失 → 回落默认值」这条隐式路径
    /// 推断，磁盘上看不出真实状态。
    pub fn ensure_backend(&self) -> AppResult<()> {
        if self.pref_path().exists() {
            return Ok(());
        }
        self.write_backend(self.backend())
    }

    /// 写入一条秘密。
    pub fn set(&self, account: &str, secret: &str) -> AppResult<()> {
        self.write_to(self.backend(), account, secret)
    }

    /// 读取一条秘密。
    ///
    /// 当前后端里没有时，会去另一个后端找一次，找到就顺手搬过来。
    /// 这样从钥匙串切到本地加密文件后，**只有第一次**要授权，
    /// 密码落到新后端后就再也不碰钥匙串了。
    ///
    /// 另一个后端读不出来（钥匙串不可用 / 没授权）时按「没有这条秘密」
    /// 处理 —— 用户选了当前后端，就不该被另一个后端连累。
    pub fn get(&self, account: &str) -> AppResult<Option<String>> {
        let backend = self.backend();
        if let Some(secret) = self.read_from(backend, account)? {
            return Ok(Some(secret));
        }

        let other = match backend {
            SecretBackend::Keyring => SecretBackend::Vault,
            SecretBackend::Vault => SecretBackend::Keyring,
        };
        let Some(secret) = self.read_optional_from(other, account) else {
            return Ok(None);
        };

        // 搬过来；搬不动也照样把这次的密码给出去，别影响连接。
        let _ = self.write_to(backend, account, &secret);
        let _ = self.delete_from(other, account);
        Ok(Some(secret))
    }

    /// 删除一条秘密。
    pub fn delete(&self, account: &str) -> AppResult<()> {
        self.delete_from(self.backend(), account)
    }

    /// 钥匙串是否可用（Linux 上可能没有 Secret Service）。
    pub fn keyring_available() -> bool {
        keyring::probe()
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
    fn backend_labels_are_stable() {
        assert_eq!(SecretBackend::Keyring.as_str(), "keyring");
        assert_eq!(SecretBackend::Vault.as_str(), "vault");
    }

    #[test]
    fn vault_backend_round_trips_secrets() {
        let dir = temp_dir("vault");
        let store = SecretStore::new(&dir);

        // 直接把后端定到 vault，避免依赖系统钥匙串。
        store.write_backend(SecretBackend::Vault).unwrap();
        assert_eq!(store.backend(), SecretBackend::Vault);

        store.set("host/password", "hunter2").unwrap();
        assert_eq!(
            store.get("host/password").unwrap().as_deref(),
            Some("hunter2")
        );

        store.delete("host/password").unwrap();
        assert!(store.get("host/password").unwrap().is_none());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_preference_falls_back_to_default() {
        let dir = temp_dir("default");
        let store = SecretStore::new(&dir);
        assert_eq!(store.backend(), store.default_backend());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn broken_preference_falls_back_to_default() {
        let dir = temp_dir("broken");
        std::fs::write(dir.join(PREF_FILE), b"{ not json").unwrap();
        let store = SecretStore::new(&dir);
        assert_eq!(store.backend(), store.default_backend());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn ensure_backend_materializes_the_current_backend() {
        let dir = temp_dir("ensure");
        let store = SecretStore::new(&dir);

        // 首次运行：偏好文件被写出来，内容就是当前后端。
        store.ensure_backend().unwrap();
        assert!(dir.join(PREF_FILE).exists(), "偏好文件应该被写出来");
        assert_eq!(store.backend(), store.default_backend());

        // 再调一次不会覆盖已有选择。
        store.write_backend(SecretBackend::Keyring).unwrap();
        store.ensure_backend().unwrap();
        assert_eq!(store.backend(), SecretBackend::Keyring);

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 当前后端里没有这条秘密时，读取应返回 `None`，**不是**报错。
    ///
    /// 这条以前在 Linux 上挂过：读不到时会退到另一个后端（系统钥匙串），
    /// 而钥匙串在没有 Secret Service 的机器上直接报错，于是一条
    /// 「本来就没有密码」的读取被搞成了失败 —— 用户选了本地加密文件，
    /// 结果连不上服务器。
    #[test]
    fn missing_secret_reads_as_none_even_if_other_backend_is_unavailable() {
        let dir = temp_dir("missing");
        let store = SecretStore::new(&dir);
        store.write_backend(SecretBackend::Vault).unwrap();

        assert_eq!(store.get("host/password").unwrap(), None);

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 另一个后端读不出来时，一律按「没有这条秘密」处理。
    #[test]
    fn unreadable_backend_is_treated_as_missing() {
        let dir = temp_dir("unreadable");
        let store = SecretStore::new(&dir);

        // macOS 上条目不存在、Linux 上没有 Secret Service —— 两种情况
        // 都必须落到 None，而不是把错误抛出去。
        assert_eq!(
            store.read_optional_from(SecretBackend::Keyring, "ssh7-unit-test-nonexistent"),
            None
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// 默认后端必须是「能用的那个」。
    ///
    /// macOS 上一律是本地加密文件；其它平台上钥匙串不可用时（Linux 上没装
    /// Secret Service）也必须退回本地加密文件 —— 否则「保存一台带密码的
    /// 主机」会直接失败，用户什么都存不了。
    #[test]
    fn default_backend_is_always_a_usable_one() {
        if cfg!(target_os = "macos") {
            assert_eq!(platform_default_backend(), SecretBackend::Vault);
        }

        let default = default_backend();
        if !keyring::probe() {
            assert_eq!(
                default,
                SecretBackend::Vault,
                "钥匙串不可用时默认后端必须是本地加密文件"
            );
        }
    }
}
