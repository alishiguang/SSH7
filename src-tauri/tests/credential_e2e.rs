//! 凭据存储的真实端到端测试。
//!
//! 默认**不参与** `cargo test`（标记了 `#[ignore]`），需要真实服务器时显式运行：
//!
//! ```sh
//! cd src-tauri
//! SSH7_E2E_HOST=10.0.80.4 \
//! SSH7_E2E_PORT=22 \
//! SSH7_E2E_USER=ssh7t \
//! SSH7_E2E_PASSWORD_FILE=/tmp/ssh7_test_pw.txt \
//! cargo test --test credential_e2e -- --ignored --nocapture --test-threads=1
//! ```
//!
//! 密码从**文件**读，不进环境变量、不进进程列表。
//!
//! 这个测试覆盖三件事：
//!
//! 1. 本地加密凭据库能存取密码，密文里不含明文；
//! 2. 两条迁移路径都工作 —— 显式切换后端时搬、惰性兜底时搬；
//! 3. 从凭据库里取出的密码能真的连上服务器。
//!
//! 钥匙串条目用**专用账号名**（`ssh7-e2e-*`），不会碰到用户自己的主机条目。

use std::path::PathBuf;
use std::sync::Arc;

use russh::client::Config;
use ssh7_lib::ssh::client::ClientHandler;
use ssh7_lib::storage::keyring;
use ssh7_lib::storage::secrets::{SecretBackend, SecretStore};

fn env_or(key: &str, fallback: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| fallback.to_string())
}

/// 临时目录。
fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ssh7-cred-e2e-{tag}-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// 从文件读密码。密码只在内存里待命，不落盘、不进环境变量、不打印。
fn password() -> String {
    let path = std::env::var("SSH7_E2E_PASSWORD_FILE").expect("缺少 SSH7_E2E_PASSWORD_FILE");
    let raw = std::fs::read_to_string(path).expect("读取密码文件失败");
    let pw = raw.trim().to_string();
    assert!(!pw.is_empty(), "密码文件是空的");
    pw
}

/// 凭据库能存取密码，且密文里不含明文。
#[test]
#[ignore = "需要真实凭据文件"]
fn vault_stores_and_reads_back_without_plaintext() {
    let dir = temp_dir("roundtrip");
    let store = SecretStore::new(&dir);
    store.set_backend(SecretBackend::Vault, &[]).unwrap();

    let account = "ssh7-e2e-roundtrip/password";
    let pw = password();
    store.set(account, &pw).unwrap();

    assert_eq!(store.get(account).unwrap().as_deref(), Some(pw.as_str()));

    // 密文里不能出现明文，也不能出现账号名。
    let blob = std::fs::read(dir.join("vault.bin")).unwrap();
    let text = String::from_utf8_lossy(&blob);
    assert!(!text.contains(&pw), "密文里不该出现明文密码");
    assert!(!text.contains(account), "密文里不该出现账号名");

    // 两个文件都必须是 0600。
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for f in ["vault.key", "vault.bin"] {
            let mode = std::fs::metadata(dir.join(f)).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "{f} 必须是 0600");
        }
    }

    // 清理钥匙串里可能残留的同名条目（本测试不写钥匙串，保险起见）。
    let _ = keyring::delete(account);
    std::fs::remove_dir_all(&dir).ok();
}

/// 显式切换后端时，已有秘密被搬过去，钥匙串里不再留副本。
#[test]
#[ignore = "需要真实凭据文件"]
fn switching_backend_migrates_existing_secrets() {
    let dir = temp_dir("switch");
    let store = SecretStore::new(&dir);

    let account = format!("ssh7-e2e-switch-{}/password", uuid::Uuid::now_v7());

    // 先放在钥匙串里。
    store.set_backend(SecretBackend::Keyring, &[]).unwrap();
    let pw = password();
    keyring::set(&account, &pw).unwrap();
    assert!(keyring::get(&account).unwrap().is_some());

    // 切到本地加密文件。
    store
        .set_backend(SecretBackend::Vault, std::slice::from_ref(&account))
        .unwrap();
    assert_eq!(store.backend(), SecretBackend::Vault);

    // 秘密已经搬到库里，钥匙串里的副本被删掉。
    assert_eq!(store.get(&account).unwrap().as_deref(), Some(pw.as_str()));
    assert!(
        keyring::get(&account).unwrap().is_none(),
        "迁移后钥匙串里不该再留副本"
    );

    let _ = keyring::delete(&account);
    std::fs::remove_dir_all(&dir).ok();
}

/// 惰性兜底：库里没有时去另一个后端找一次，找到就顺手搬回来。
#[test]
#[ignore = "需要真实凭据文件"]
fn lazy_fallback_migrates_secret_found_in_other_backend() {
    let dir = temp_dir("lazy");
    let store = SecretStore::new(&dir);

    let account = format!("ssh7-e2e-lazy-{}/password", uuid::Uuid::now_v7());

    // 秘密只在钥匙串里，而当前后端是 vault。
    store.set_backend(SecretBackend::Keyring, &[]).unwrap();
    let pw = password();
    keyring::set(&account, &pw).unwrap();
    store.set_backend(SecretBackend::Vault, &[]).unwrap();

    // 库里没有 → 应该去钥匙串找 → 找到后搬进库里。
    assert_eq!(store.get(&account).unwrap().as_deref(), Some(pw.as_str()));
    assert!(
        keyring::get(&account).unwrap().is_none(),
        "惰性迁移后钥匙串里不该再留副本"
    );

    // 再读一次应该直接命中库里，不再碰钥匙串。
    assert_eq!(store.get(&account).unwrap().as_deref(), Some(pw.as_str()));

    let _ = keyring::delete(&account);
    std::fs::remove_dir_all(&dir).ok();
}

/// 从凭据库里取出的密码，能真的连上服务器。
#[tokio::test]
#[ignore = "需要真实 SSH 服务器"]
async fn credential_from_vault_authenticates_against_real_server() {
    let dir = temp_dir("connect");
    let store = SecretStore::new(&dir);
    store.set_backend(SecretBackend::Vault, &[]).unwrap();

    let host = env_or("SSH7_E2E_HOST", "10.0.80.4");
    let port: u16 = env_or("SSH7_E2E_PORT", "22").parse().expect("端口号");
    let user = env_or("SSH7_E2E_USER", "root");

    let account = format!("ssh7-e2e-connect-{}/password", uuid::Uuid::now_v7());
    let pw = password();

    // 模拟 App 的保存流程：密码进凭据库。
    store.set(&account, &pw).unwrap();

    // 模拟 App 的连接流程：凭据从库里取（而不是从钥匙串）。
    let credential = store.get(&account).unwrap().expect("凭据库里应该有密码");

    let handler = ClientHandler {
        host: host.clone(),
        port,
    };
    let config = Arc::new(Config::default());
    let mut handle = russh::client::connect(config, (host.as_str(), port), handler)
        .await
        .expect("SSH 连接失败");
    let auth = handle
        .authenticate_password(&user, &credential)
        .await
        .expect("认证请求失败");

    assert!(auth.success(), "认证被拒绝");
    println!("用凭据库里的密码连上 {user}@{host}:{port} 成功");

    std::fs::remove_dir_all(&dir).ok();
}

/// 在**真实应用配置目录**上跑一遍：密码写进去、读出来，密文里没有明文。
///
/// 临时目录只能证明逻辑对，证明不了「应用真正用的那个目录」也对。
/// 用 `SSH7_E2E_APP_CONFIG_DIR` 指向应用配置目录即可，例如：
///
/// ```sh
/// SSH7_E2E_APP_CONFIG_DIR="$HOME/Library/Application Support/com.alishiguang.ssh7" \
/// cargo test --test credential_e2e -- --ignored --nocapture
/// ```
#[test]
#[ignore = "需要真实应用配置目录"]
fn real_app_config_dir_round_trips_without_plaintext() {
    let Ok(dir) = std::env::var("SSH7_E2E_APP_CONFIG_DIR") else {
        eprintln!("跳过：没给 SSH7_E2E_APP_CONFIG_DIR");
        return;
    };
    let dir = PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();

    let store = SecretStore::new(&dir);
    store.ensure_backend().unwrap();
    println!("真实配置目录的后端：{}", store.backend().as_str());

    let account = format!("ssh7-e2e-real-{}/password", uuid::Uuid::now_v7());
    let pw = password();
    store.set(&account, &pw).unwrap();
    assert_eq!(store.get(&account).unwrap().as_deref(), Some(pw.as_str()));

    // 密文里不该出现明文。
    let blob = std::fs::read(dir.join("vault.bin")).unwrap();
    assert!(
        !String::from_utf8_lossy(&blob).contains(&pw),
        "真实配置目录的凭据库里不该出现明文"
    );

    // 主密钥与密文都必须是 0600。
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for f in ["vault.key", "vault.bin"] {
            let mode = std::fs::metadata(dir.join(f)).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "{f} 必须是 0600");
        }
    }

    // 清理测试条目，别在用户自己的库里留东西。
    store.delete(&account).unwrap();
    assert!(store.get(&account).unwrap().is_none());
    println!("真实配置目录往返正常，测试条目已清理");
}
