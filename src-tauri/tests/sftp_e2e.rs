//! 真实服务器端到端测试。
//!
//! 默认**不参与** `cargo test`（标记了 `#[ignore]`），需要真实服务器时显式运行：
//!
//! ```sh
//! cd src-tauri
//! SSH7_E2E_HOST=10.0.80.4 \
//! SSH7_E2E_PORT=22 \
//! SSH7_E2E_USER=root \
//! SSH7_E2E_HOST_ID=01a20ce2-32ab-7184-8683-2eec1f61bfa0 \
//! SSH7_E2E_APP_CONFIG_DIR="$HOME/Library/Application Support/com.chuaker7.ssh7" \
//! cargo test --test sftp_e2e -- --ignored --nocapture
//! ```
//!
//! 认证方式三选一：
//!
//! - `SSH7_E2E_KEY_PATH`：用私钥认证（推荐，顺带覆盖 App 的私钥解析路径）；
//! - `SSH7_E2E_HOST_ID` + `SSH7_E2E_APP_CONFIG_DIR`：从应用自己的**加密凭据库**
//!   （`vault.bin`）取密码 —— 跟 App 里走的是同一条路；
//! - `SSH7_E2E_KEYCHAIN_ACCOUNT`：从系统钥匙串取密码（老办法，保留是为了
//!   在没配 vault 时也能跑）。
//!
//! 都没给时，测试会被跳过而不是失败。

use std::sync::{Arc, OnceLock};

use russh::client::Config;
use russh::keys::PrivateKeyWithHashAlg;
use ssh7_lib::ssh::auth::{load_private_key, Credential};
use ssh7_lib::ssh::client::ClientHandler;
use ssh7_lib::ssh::sftp::{plan_upload, SftpClient};
use ssh7_lib::storage::secrets;
use ssh7_lib::storage::secrets::SecretStore;

/// 读到的密码只在本进程内存里待命，不落盘、不进环境变量、不打印。
static PASSWORD: OnceLock<String> = OnceLock::new();

/// 从应用自己的加密凭据库里取密码 —— 跟 App 里走的是同一条路。
///
/// v1.8.0 起 SSH7 只往加密文件里写密码，钥匙串那套已经删了；这个测试要是
/// 还只认钥匙串，就等于永远跑不起来。
fn vault_password(host_id: &str) -> Option<String> {
    let dir = std::env::var("SSH7_E2E_APP_CONFIG_DIR").ok()?;
    let store = SecretStore::new(std::path::Path::new(&dir));
    let account = secrets::password_account(host_id);
    store.get(&account).ok().flatten()
}

/// 读取测试用密码。
///
/// SSH7 自己只往加密文件里写密码，所以这里直接从**应用配置目录**里读不到 ——
/// 测试就走 macOS 的 `security` CLI，或者用 `SSH7_E2E_PASSWORD_FILE` 指定。
fn keychain_password(account: &str) -> Option<String> {
    if let Some(v) = PASSWORD.get() {
        return Some(v.clone());
    }

    let secret = Some(())
        .and(None)
        .or_else(|| {
            let out = std::process::Command::new("security")
                .args(["find-generic-password", "-s", "ssh7", "-a", account, "-w"])
                .output()
                .ok()?;
            if !out.status.success() {
                return None;
            }
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            (!s.is_empty()).then_some(s)
        })
        .filter(|s| !s.is_empty())?;

    Some(PASSWORD.get_or_init(|| secret).clone())
}

fn env_or(key: &str, fallback: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| fallback.to_string())
}

/// 决定用哪种凭据连接。没有可用凭据时返回 `None`（测试跳过）。
fn credential() -> Option<Credential> {
    if let Ok(path) = std::env::var("SSH7_E2E_KEY_PATH") {
        let key = load_private_key(&path, None).expect("私钥解析失败");
        return Some(Credential::PrivateKey { key });
    }
    // 跟 App 走同一条路：从应用自己的加密凭据库里取。
    if let Ok(host_id) = std::env::var("SSH7_E2E_HOST_ID") {
        if let Some(password) = vault_password(&host_id) {
            return Some(Credential::Password(password));
        }
    }
    let account = std::env::var("SSH7_E2E_KEYCHAIN_ACCOUNT").ok()?;
    let password = keychain_password(&account)?;
    Some(Credential::Password(password))
}

/// 建立一条带 SFTP 子系统的真实连接。
async fn connect_sftp() -> (Arc<SftpClient>, String) {
    let host = env_or("SSH7_E2E_HOST", "10.0.80.4");
    let port: u16 = env_or("SSH7_E2E_PORT", "22").parse().expect("端口号");
    let user = env_or("SSH7_E2E_USER", "root");

    let credential = credential().expect(
        "缺少凭据：请设置 SSH7_E2E_KEY_PATH（私钥路径）、SSH7_E2E_HOST_ID + SSH7_E2E_APP_CONFIG_DIR（从加密凭据库取），或 SSH7_E2E_KEYCHAIN_ACCOUNT（钥匙串账号）",
    );

    let handler = ClientHandler {
        host: host.clone(),
        port,
    };
    let config = Arc::new(Config::default());
    let mut handle = russh::client::connect(config, (host.as_str(), port), handler)
        .await
        .expect("SSH 连接失败");

    let auth = match &credential {
        Credential::Password(pw) => handle
            .authenticate_password(&user, pw.as_str())
            .await
            .expect("认证请求失败"),
        Credential::PrivateKey { key } => {
            let alg = PrivateKeyWithHashAlg::new(key.clone(), None);
            handle
                .authenticate_publickey(&user, alg)
                .await
                .expect("认证请求失败")
        }
    };
    assert!(auth.success(), "认证被拒绝");

    let sftp = SftpClient::open(Arc::new(tokio::sync::Mutex::new(handle)))
        .await
        .expect("SFTP 子系统启动失败");

    (sftp, host)
}

/// 根因回归：SFTP channel 必须先请求 `sftp` 子系统，否则列目录必然失败。
#[tokio::test]
#[ignore = "需要真实 SSH 服务器"]
async fn lists_remote_root_over_sftp() {
    let (sftp, _host) = connect_sftp().await;

    let files = sftp.list("/").await.expect("列远端根目录失败");
    assert!(!files.is_empty(), "/ 至少应该有一些条目");

    let names: Vec<&str> = files.iter().take(5).map(|f| f.name.as_str()).collect();
    println!("远端 / 共 {} 个条目，示例：{names:?}", files.len());
}

/// 相对路径 `"."` 必须被解析成远端绝对路径。
#[tokio::test]
#[ignore = "需要真实 SSH 服务器"]
async fn resolves_relative_target_directory() {
    let (sftp, _host) = connect_sftp().await;

    let resolved = sftp.resolve_remote_dir(".").await;
    assert!(
        resolved.starts_with('/'),
        "解析结果应为绝对路径，实际是 {resolved}"
    );
    println!("\".\" 解析为 {resolved}");
}

/// 递归目录上传：远端目录结构与文件内容都要对得上。
#[tokio::test]
#[ignore = "需要真实 SSH 服务器"]
async fn uploads_a_directory_tree_recursively() {
    let (sftp, _host) = connect_sftp().await;

    // 本地造一棵带嵌套的目录树。
    let local_root = std::env::temp_dir().join(format!("ssh7-e2e-{}", uuid::Uuid::now_v7()));
    let project = local_root.join("proj");
    std::fs::create_dir_all(project.join("sub/deep")).unwrap();
    std::fs::write(project.join("top.txt"), b"top-content").unwrap();
    std::fs::write(project.join("sub/mid.txt"), b"mid-content").unwrap();
    std::fs::write(project.join("sub/deep/low.txt"), b"low-content").unwrap();

    let plan = plan_upload(&project).expect("生成上传计划失败");
    assert_eq!(plan.dirs, vec!["proj", "proj/sub", "proj/sub/deep"]);
    assert_eq!(plan.total, 3 * 11);

    // 上传到远端临时目录（目录本身尚不存在，也应该被自动创建）。
    let remote_base = format!("/tmp/ssh7-e2e-{}", uuid::Uuid::now_v7());
    sftp.apply_plan(&plan, &remote_base, |_, _, _| {})
        .await
        .expect("上传失败");

    // 远端结构应当与本地一致。
    let top = sftp.list(&remote_base).await.expect("读取远端目录失败");
    assert!(top.iter().any(|f| f.name == "proj" && f.is_directory));

    let sub = sftp
        .list(&format!("{remote_base}/proj/sub"))
        .await
        .expect("读取远端子目录失败");
    assert!(sub.iter().any(|f| f.name == "mid.txt" && f.size == 11));

    let deep = sftp
        .list(&format!("{remote_base}/proj/sub/deep"))
        .await
        .expect("读取远端深层目录失败");
    assert!(deep.iter().any(|f| f.name == "low.txt" && f.size == 11));

    // 清理远端临时目录。
    let _ = sftp.remove(&remote_base, true).await;
    let _ = std::fs::remove_dir_all(&local_root);
}

/// 递归删除：目录里的内容也要一起清掉。
#[tokio::test]
#[ignore = "需要真实 SSH 服务器"]
async fn removes_a_directory_recursively() {
    let (sftp, _host) = connect_sftp().await;

    let remote_base = format!("/tmp/ssh7-e2e-del-{}", uuid::Uuid::now_v7());
    // POSIX 语义：mkdir 不会顺带创建父目录，逐级建。
    sftp.mkdir(&remote_base).await.expect("创建远端目录失败");
    sftp.mkdir(&format!("{remote_base}/a"))
        .await
        .expect("创建远端子目录失败");
    sftp.mkdir(&format!("{remote_base}/a/b"))
        .await
        .expect("创建远端深层目录失败");
    sftp.mkdir(&format!("{remote_base}/a/b/c"))
        .await
        .expect("创建远端更深层目录失败");

    sftp.remove(&remote_base, true).await.expect("递归删除失败");

    let err = sftp.list(&remote_base).await;
    assert!(err.is_err(), "删除后不应还能列出该目录");
}

/// 上传后原样下载回来，内容必须一致。
#[tokio::test]
#[ignore = "需要真实 SSH 服务器"]
async fn downloads_a_file_back_unchanged() {
    let (sftp, _host) = connect_sftp().await;

    // 造一个内容有辨识度的文件。
    let payload: Vec<u8> = (0u8..=255).cycle().take(300_000).collect();
    let local_root = std::env::temp_dir().join(format!("ssh7-e2e-dl-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&local_root).unwrap();
    let local_file = local_root.join("blob.bin");
    std::fs::write(&local_file, &payload).unwrap();

    // 上传。
    let plan = plan_upload(&local_file).unwrap();
    let remote_base = format!("/tmp/ssh7-e2e-dl-{}", uuid::Uuid::now_v7());
    sftp.apply_plan(&plan, &remote_base, |_, _, _| {})
        .await
        .expect("上传失败");

    // 下载回本地另一个目录。
    let back_dir = std::env::temp_dir().join(format!("ssh7-e2e-back-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&back_dir).unwrap();
    let saved = sftp
        .download_to(
            &format!("{remote_base}/blob.bin"),
            back_dir.to_str().unwrap(),
            |_, _, _| {},
        )
        .await
        .expect("下载失败");

    let downloaded = std::fs::read(&saved).unwrap();
    assert_eq!(downloaded.len(), payload.len(), "下载字节数不一致");
    assert_eq!(downloaded, payload, "下载内容与原文件不一致");

    // 清理。
    let _ = sftp.remove(&remote_base, true).await;
    let _ = std::fs::remove_dir_all(&local_root);
    let _ = std::fs::remove_dir_all(&back_dir);
}
