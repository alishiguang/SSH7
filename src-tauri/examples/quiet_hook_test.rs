//! 端到端验证「静默安装 OSC 7 钩子」的完整方案。
//!
//! 做法：
//! 1. `pty-req` 带上 `(Pty::ECHO, 0)` —— 远端 PTY 从一开始就不回显；
//! 2. 把「装钩子 + 擦掉提示符 + 恢复回显」写成**一条**命令发过去：
//!    `<钩子>; printf '\r\033[K'; stty echo`
//!    —— 这条命令本身既不会被内核回显，readline 也不会显示；
//! 3. `printf '\r\033[K'` 把多出来的提示符行擦掉，shell 随后重打一次提示符，
//!    屏幕上看起来跟普通会话一模一样。
//!
//! 判定标准：
//! - 钩子代码一个字都不能出现在屏幕上；
//! - 提示符只出现一次；
//! - OSC 7 序列照常发出（cd 之后 cwd 能跟上）；
//! - 恢复回显后用户输入正常显示。
//!
//! 用法：
//! ```sh
//! SSH7_E2E_HOST=10.0.80.4 SSH7_E2E_USER=root \
//! SSH7_E2E_APP_CONFIG_DIR="$HOME/Library/Application Support/com.alishiguang.ssh7" \
//! cargo run --example quiet_hook_test
//! ```

use std::sync::Arc;
use std::time::Duration;

use russh::client::Config;
use russh::Pty;

use ssh7_lib::ssh::client::ClientHandler;
use ssh7_lib::ssh::tracker::{self, ShellKind};
use ssh7_lib::storage::keyring;
use ssh7_lib::storage::secrets::SecretStore;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let host = std::env::var("SSH7_E2E_HOST").unwrap_or_else(|_| "10.0.80.4".into());
    let port: u16 = std::env::var("SSH7_E2E_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(22);
    let user = std::env::var("SSH7_E2E_USER").unwrap_or_else(|_| "root".into());
    let dir = std::env::var("SSH7_E2E_APP_CONFIG_DIR").map(std::path::PathBuf::from)?;

    let store = SecretStore::new(&dir);
    let account = keyring::password_account("01a20ce2-32ab-7184-8683-2eec1f61bfa0");
    let password = store.get(&account)?.ok_or("没有密码")?;

    let config = Arc::new(Config {
        inactivity_timeout: Some(Duration::from_secs(60)),
        ..Default::default()
    });
    let handler = ClientHandler {
        host: host.clone(),
        port,
    };
    let mut handle = russh::client::connect(config, (host.as_str(), port), handler).await?;
    handle
        .authenticate_password(&user, password.as_str())
        .await?;

    let channel = handle.channel_open_session().await?;
    channel
        .request_pty(true, "xterm-256color", 80, 24, 0, 0, &[(Pty::ECHO, 0)])
        .await?;
    channel.request_shell(true).await?;
    let (mut read_half, writer) = channel.split();

    // 启动横幅 + 第一个提示符
    let banner = collect(&mut read_half, Duration::from_millis(1500)).await;
    println!("=== 启动横幅 ===\n{banner}\n");

    // ---- 静默安装钩子 ----
    let install = tracker::install_command(ShellKind::Bash);
    writer
        .data_bytes(bytes::Bytes::from(install.into_bytes()))
        .await?;
    let after = collect(&mut read_half, Duration::from_millis(1500)).await;
    println!("=== 安装钩子后的输出 ===\n{after}\n");

    let visible_hook = after.contains("__ssh7_osc7");
    let prompt_count = after.matches(":~#").count();
    let osc7 = after.contains("\x1b]7;");

    println!("  钩子代码出现在屏幕上: {visible_hook}  （应为 false）");
    println!("  提示符出现次数: {prompt_count}  （应为 1）");
    println!("  OSC 7 序列已发出: {osc7}  （应为 true）");
    println!(
        "  安装命令: {:?}",
        tracker::install_command(ShellKind::Bash)
    );

    // ---- 模拟用户 cd，看 cwd 是否跟上 ----
    writer.data_bytes(bytes::Bytes::from("cd /tmp\n")).await?;
    let after_cd = collect(&mut read_half, Duration::from_millis(1500)).await;
    println!("=== cd /tmp 后的输出 ===\n{after_cd}\n");
    println!(
        "  用户输入可见: {}  （应为 true）",
        after_cd.contains("cd /tmp")
    );
    println!(
        "  cwd 跟到 /tmp: {}  （应为 true）",
        after_cd.contains("\x1b]7;file://") && after_cd.contains("/tmp")
    );

    let ok = !visible_hook && prompt_count == 1 && osc7;
    println!(
        "\n{}",
        if ok {
            "✅ 方案成立"
        } else {
            "❌ 方案不成立"
        }
    );
    Ok(())
}

async fn collect(read_half: &mut russh::ChannelReadHalf, dur: Duration) -> String {
    let deadline = tokio::time::Instant::now() + dur;
    let mut out = String::new();
    while tokio::time::Instant::now() < deadline {
        if let Ok(Some(russh::ChannelMsg::Data { data })) =
            tokio::time::timeout(Duration::from_millis(300), read_half.wait()).await
        {
            out.push_str(&String::from_utf8_lossy(&data));
        }
    }
    out
}
