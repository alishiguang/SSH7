//! 维护工具：把系统钥匙串里的密码搬进本地加密凭据库。
//!
//! macOS 上未签名的应用读取钥匙串条目会弹窗索要「登录」钥匙串密码。
//! 运行一次本工具把密码搬进加密库之后，SSH7 就再也不碰钥匙串了 ——
//! 连第一次连接都不会弹。
//!
//! 用法：
//!
//! ```sh
//! cargo run --example vaultmigrate -- <host_id>
//! ```
//!
//! `<host_id>` 是主机在 `hosts.json` 里的 id（界面上看不到，可以打开
//! `hosts.json` 看）。不带参数时会列出 `hosts.json` 里所有主机及其 id。
//!
//! 注意：读取钥匙串那一步可能弹窗要求授权 —— 那是最后一次。授权之后
//! 密码就落进加密库，钥匙串里的副本会被删掉。

use std::path::PathBuf;

use ssh7_lib::storage::keyring;
use ssh7_lib::storage::secrets::SecretStore;

/// 应用的配置目录。macOS 上是 `~/Library/Application Support/com.alishiguang.ssh7`。
fn config_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("SSH7_CONFIG_DIR") {
        return PathBuf::from(dir);
    }
    let home = std::env::var("HOME").expect("拿不到 HOME");
    PathBuf::from(home).join("Library/Application Support/com.alishiguang.ssh7")
}

fn main() {
    let dir = config_dir();
    let store = SecretStore::new(&dir);
    store.ensure_backend().expect("写凭据后端偏好失败");

    println!("配置目录：{}", dir.display());
    println!("当前后端：{}", store.backend().as_str());

    let args: Vec<String> = std::env::args().collect();
    let Some(host_id) = args.get(1).cloned() else {
        println!("\n请给出主机 id。hosts.json 里现有的主机：");
        let hosts_path = dir.join("hosts.json");
        match std::fs::read_to_string(&hosts_path) {
            Ok(raw) => {
                // 简单扫一遍 id 与标签，不引额外依赖。
                for line in raw.lines() {
                    let line = line.trim();
                    if let Some(rest) = line.strip_prefix("\"id\":") {
                        println!("  id = {}", rest.trim().trim_end_matches(','));
                    } else if let Some(rest) = line.strip_prefix("\"label\":") {
                        println!("      标签 = {}", rest.trim().trim_end_matches(','));
                    }
                }
            }
            Err(e) => println!("  读不到 hosts.json：{e}"),
        }
        println!("\n用法：cargo run --example vaultmigrate -- <host_id>");
        return;
    };

    for (kind, account) in [
        ("密码", keyring::password_account(&host_id)),
        ("私钥口令", keyring::passphrase_account(&host_id)),
    ] {
        // 这一步可能要弹窗索要「登录」钥匙串密码 —— 授权之后就不会再弹了。
        let Some(secret) = keyring::get(&account).unwrap_or_else(|e| {
            println!("读取{kind}失败：{e}");
            None
        }) else {
            println!("钥匙串里没有这台主机的{kind}，跳过");
            continue;
        };

        store
            .set(&account, &secret)
            .unwrap_or_else(|e| panic!("写入加密库失败：{e}"));
        keyring::delete(&account).ok();

        println!(
            "✓ {kind}已搬进加密库（{} 字节），钥匙串里的副本已删除",
            secret.len()
        );
    }

    println!("\n完成。SSH7 之后不会再访问系统钥匙串。");
}
