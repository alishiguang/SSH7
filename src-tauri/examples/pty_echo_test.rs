//! 验证完整方案：pty-req 带 ECHO=0 → 注入脚本完全不可见 → `stty echo` 恢复后
//! 用户输入重新可见。
//!
//! 步骤：
//! 1. pty-req 带上 `(Pty::ECHO, 0)`，远端 PTY 一开始就不回显；
//! 2. 写入一条命令 —— 应该**完全看不见**（内核不回显，readline 也不显示）；
//! 3. 写入 `stty echo` —— 也应该看不见；
//! 4. 再写一条命令 —— 这次应该**看得见**（模拟用户输入），且只出现一次。
//!
//! 用法：
//! ```sh
//! SSH7_E2E_HOST=10.0.80.4 SSH7_E2E_USER=root \
//! SSH7_E2E_APP_CONFIG_DIR="$HOME/Library/Application Support/com.chuaker7.ssh7" \
//! cargo run --example pty_echo_test
//! ```

use std::sync::Arc;
use std::time::Duration;

use russh::client::Config;
use russh::Pty;

use ssh7_lib::ssh::client::ClientHandler;
use ssh7_lib::storage::secrets;
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
    let account = secrets::password_account("01a20ce2-32ab-7184-8683-2eec1f61bfa0");
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
    // 关键：把 ECHO 关掉。
    channel
        .request_pty(true, "xterm-256color", 80, 24, 0, 0, &[(Pty::ECHO, 0)])
        .await?;
    channel.request_shell(true).await?;

    let (mut read_half, writer) = channel.split();

    // 读掉启动横幅与第一个提示符

    let _ = tokio::time::timeout(Duration::from_millis(1500), read_half.wait()).await;

    // ---- 第 1 步：注入（应当不可见）----
    let inject = "__ssh7_probe() { :; }; echo INJECTED_HIDDEN_AAA1\n";
    writer
        .data_bytes(bytes::Bytes::from(inject.as_bytes()))
        .await?;
    let seen1 = collect(&mut read_half, Duration::from_millis(1500)).await;
    println!("=== 第 1 步（注入）收到的输出 ===\n{seen1}\n");
    println!(
        "  'INJECTED_HIDDEN_AAA1' 出现 {} 次 —— 命令本身{}",
        seen1.matches("INJECTED_HIDDEN_AAA1").count(),
        if seen1.matches("echo INJECTED_HIDDEN_AAA1").count() == 0 {
            "没有被回显 ✓"
        } else {
            "被回显了 ✗"
        }
    );

    // ---- 第 2 步：恢复回显（应当不可见）----
    writer.data_bytes(bytes::Bytes::from("stty echo\n")).await?;
    let seen2 = collect(&mut read_half, Duration::from_millis(1200)).await;
    println!("=== 第 2 步（stty echo）收到的输出 ===\n{seen2}\n");
    println!(
        "  'stty echo' 被回显 {} 次 —— {}",
        seen2.matches("stty echo").count(),
        if seen2.matches("stty echo").count() == 0 {
            "不可见 ✓"
        } else {
            "可见 ✗"
        }
    );

    // ---- 第 3 步：模拟用户输入（应当可见且只出现一次）----
    writer
        .data_bytes(bytes::Bytes::from("echo USER_TYPED_BBB2\n"))
        .await?;
    let seen3 = collect(&mut read_half, Duration::from_millis(1500)).await;
    println!("=== 第 3 步（用户输入）收到的输出 ===\n{seen3}\n");
    let total = seen3.matches("USER_TYPED_BBB2").count();
    let echoed = seen3.matches("echo USER_TYPED_BBB2").count();
    println!("  标记总出现 {total} 次，其中作为回显 {echoed} 次");
    println!(
        "  结论：{}",
        if echoed == 1 {
            "用户输入正常显示且只显示一次 ✓"
        } else if echoed == 0 {
            "用户输入不可见 ✗（会让人瞎敲）"
        } else {
            "用户输入被显示多次 ✗（双重回显）"
        }
    );
    Ok(())
}

/// 读一段时间的输出，返回累计文本。
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
