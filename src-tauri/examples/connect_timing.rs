//! 连接耗时诊断：逐段测量「点一下主机」到底慢在哪。
//!
//! 用法：
//! ```sh
//! SSH7_E2E_HOST=10.0.80.4 SSH7_E2E_PORT=22 SSH7_E2E_USER=root \
//! SSH7_E2E_APP_CONFIG_DIR="$HOME/Library/Application Support/com.alishiguang.ssh7" \
//! cargo run --example connect_timing
//! ```

use std::sync::Arc;
use std::time::{Duration, Instant};

use russh::client::{Config, Handle};
use russh::keys::PrivateKeyWithHashAlg;
use russh::ChannelMsg;
use tokio::io::AsyncReadExt;

use ssh7_lib::ssh::auth::Credential;
use ssh7_lib::ssh::client::ClientHandler;
use ssh7_lib::storage::keyring;
use ssh7_lib::storage::secrets::SecretStore;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(run())
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let host = std::env::var("SSH7_E2E_HOST").unwrap_or_else(|_| "10.0.80.4".into());
    let port: u16 = std::env::var("SSH7_E2E_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(22);
    let user = std::env::var("SSH7_E2E_USER").unwrap_or_else(|_| "root".into());
    let dir = std::env::var("SSH7_E2E_APP_CONFIG_DIR").map(std::path::PathBuf::from)?;

    // ---- 凭据读取 ----
    let store = SecretStore::new(&dir);
    let account = keyring::password_account("01a20ce2-32ab-7184-8683-2eec1f61bfa0");
    let t = Instant::now();
    let password = store.get(&account)?;
    println!(
        "凭据读取 ({:?}): {:.1} ms  拿到={}",
        store.backend(),
        t.elapsed().as_secs_f64() * 1000.0,
        password.is_some()
    );

    let credential = match password {
        Some(p) => Credential::Password(p),
        None => return Err("没有密码".into()),
    };

    let start = Instant::now();
    let mut prev = Instant::now();
    macro_rules! lap {
        ($label:expr) => {{
            let now = Instant::now();
            println!(
                "  {:<28} {:>8.1} ms   (累计 {:>8.1} ms)",
                $label,
                now.duration_since(prev).as_secs_f64() * 1000.0,
                now.duration_since(start).as_secs_f64() * 1000.0
            );
            now
        }};
    }

    // ---- TCP + 握手 ----
    let config = Arc::new(Config {
        inactivity_timeout: Some(Duration::from_secs(3600)),
        keepalive_interval: Some(Duration::from_secs(30)),
        keepalive_max: 3,
        nodelay: true,
        ..Default::default()
    });
    let handler = ClientHandler {
        host: host.clone(),
        port,
    };
    let mut handle: Handle<ClientHandler> =
        russh::client::connect(config, (host.as_str(), port), handler).await?;
    prev = lap!("TCP + SSH 握手");

    // ---- 认证 ----
    let auth = match &credential {
        Credential::Password(pw) => handle.authenticate_password(&user, pw.as_str()).await?,
        Credential::PrivateKey { key } => {
            handle
                .authenticate_publickey(&user, PrivateKeyWithHashAlg::new(key.clone(), None))
                .await?
        }
    };
    println!("  认证成功: {}", auth.success());
    prev = lap!("认证");

    // ---- channel + pty + shell ----
    let channel = handle.channel_open_session().await?;
    prev = lap!("channel_open_session");
    channel
        .request_pty(true, "xterm-256color", 80, 24, 0, 0, &[])
        .await?;
    prev = lap!("request_pty");
    channel.request_shell(true).await?;
    prev = lap!("request_shell");

    // ---- 探测 shell（额外开一个 exec channel）----
    let detect = handle.channel_open_session().await?;
    detect.exec(true, "echo $0").await?;
    let mut stream = detect.into_stream();
    let mut buf = Vec::new();
    let read = tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut buf)).await;
    let got = String::from_utf8_lossy(&buf).trim().to_string();
    println!("  探测结果: {got:?} 超时={}", read.is_err());
    prev = lap!("探测 shell（exec channel）");

    // ---- 注入钩子后，首个输出到达时间 ----
    let channel2 = handle.channel_open_session().await?;
    channel2
        .request_pty(true, "xterm-256color", 80, 24, 0, 0, &[])
        .await?;
    channel2.request_shell(true).await?;
    let (mut read2, _writer2) = channel2.split();
    let t_out = Instant::now();
    let mut first = false;
    while let Ok(Some(msg)) = tokio::time::timeout(Duration::from_secs(3), read2.wait()).await {
        if let ChannelMsg::Data { .. } = msg {
            first = true;
            break;
        }
    }
    println!(
        "  首个终端输出: {} {:.1} ms",
        first,
        t_out.elapsed().as_secs_f64() * 1000.0
    );
    prev = lap!("等首个终端输出");
    let _ = prev;

    println!(
        "\n总计（不含凭据读取）: {:.1} ms",
        start.elapsed().as_secs_f64() * 1000.0
    );
    Ok(())
}
