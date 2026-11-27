//! 「注入钩子不污染远端 history」的真实端到端测试。
//!
//! 缺陷 #42：钩子是通过 PTY **输入流**发给远端 shell 的 —— shell 把它当成
//! 用户亲手敲的命令执行，于是这两行进了用户的 history。用户在自己的服务器上
//! 执行 `history`，就能看到 `__ssh7_osc7`、`stty echo` 这种看不懂的东西
//! （而且每开一次会话就多两条）。
//!
//! 屏幕上看不见 ≠ 没进 history —— 这是两回事。
//!
//! 这个测试到真服务器上验证三件事：
//!
//! 1. 装完钩子，远端 `history` 里**看不到**我们的命令；
//! 2. 钩子代码依然一个字都不出现在终端输出里（缺陷 #21 不能回退）；
//! 3. 工作目录跟踪照常工作（OSC 7 照常发出）。
//!
//! 需要真实服务器时显式运行：
//!
//! ```sh
//! cd src-tauri
//! SSH7_E2E_HOST=10.0.80.4 SSH7_E2E_PORT=22 SSH7_E2E_USER=root \
//! SSH7_E2E_HOST_ID=01a20ce2-32ab-7184-8683-2eec1f61bfa0 \
//! SSH7_E2E_APP_CONFIG_DIR="$HOME/Library/Application Support/com.chuaker7.ssh7" \
//! cargo test --test history_clean_e2e -- --ignored --nocapture --test-threads=1
//! ```

use std::sync::Arc;
use std::time::Duration;

use russh::client::Config;
use russh::Pty;

use ssh7_lib::ssh::client::ClientHandler;
use ssh7_lib::ssh::tracker::{self, ShellKind};
use ssh7_lib::storage::secrets;
use ssh7_lib::storage::secrets::SecretStore;

/// 与生产代码一致：关掉远端 PTY 的回显。
const PTY_MODES: &[(Pty, u32)] = &[(Pty::ECHO, 0)];

fn env_or(key: &str, fallback: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| fallback.to_string())
}

/// 读一段时间的终端输出。
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

/// 装完钩子之后，远端 history 里不能出现我们的内部命令。
#[tokio::test]
#[ignore = "需要真实服务器"]
async fn hook_install_leaves_no_trace_in_remote_history() {
    let host = env_or("SSH7_E2E_HOST", "10.0.80.4");
    let port: u16 = env_or("SSH7_E2E_PORT", "22").parse().unwrap();
    let user = env_or("SSH7_E2E_USER", "root");
    let dir = std::env::var("SSH7_E2E_APP_CONFIG_DIR")
        .map(std::path::PathBuf::from)
        .expect("缺少 SSH7_E2E_APP_CONFIG_DIR");

    // 凭据从本地加密库取，跟应用里走的是同一条路。
    let store = SecretStore::new(&dir);
    let account = secrets::password_account(&env_or("SSH7_E2E_HOST_ID", ""));
    let password = store
        .get(&account)
        .expect("读取凭据失败")
        .expect("凭据库里没有这条密码");

    let config = Arc::new(Config {
        inactivity_timeout: Some(Duration::from_secs(60)),
        ..Default::default()
    });
    let handler = ClientHandler {
        host: host.clone(),
        port,
    };
    let mut handle = russh::client::connect(config, (host.as_str(), port), handler)
        .await
        .expect("连接失败");
    handle
        .authenticate_password(&user, password.as_str())
        .await
        .expect("认证失败");

    let channel = handle
        .channel_open_session()
        .await
        .expect("开 channel 失败");
    channel
        .request_pty(true, "xterm-256color", 80, 24, 0, 0, PTY_MODES)
        .await
        .expect("申请 pty 失败");
    channel.request_shell(true).await.expect("申请 shell 失败");
    let (mut read_half, writer) = channel.split();

    // 启动横幅 + 第一个提示符
    let _ = collect(&mut read_half, Duration::from_millis(1500)).await;

    // 生产流程：先在独立的 exec channel 上探测 shell（不碰交互式终端），
    // 再按探测结果装钩子。
    let detect = handle
        .channel_open_session()
        .await
        .expect("开 exec channel 失败");
    detect
        .exec(true, tracker::DETECT_COMMAND)
        .await
        .expect("执行探测命令失败");
    let mut stream = detect.into_stream();
    let mut raw = Vec::new();
    tokio::io::AsyncReadExt::read_to_end(&mut stream, &mut raw)
        .await
        .expect("读取探测结果失败");
    let detected = tracker::parse_detect_output(&String::from_utf8_lossy(&raw));
    println!("探测到的远端 shell: {detected:?}");

    // 生产代码用的那几行命令
    let command = tracker::install_command(detected);
    println!("注入的命令:\n{command}");
    writer
        .data_bytes(bytes::Bytes::from(command.into_bytes()))
        .await
        .expect("写入钩子失败");
    let after = collect(&mut read_half, Duration::from_millis(1500)).await;

    // 缺陷 #21 不能回退：屏幕上依然一个字都不留。
    assert!(
        !after.contains("__ssh7_osc7"),
        "钩子代码出现在了屏幕上：\n{after}"
    );
    assert!(
        !after.contains("stty echo"),
        "恢复回显的命令出现在了屏幕上：\n{after}"
    );
    assert!(
        after.contains("\x1b]7;"),
        "OSC 7 没有发出，工作目录跟踪坏了：\n{after}"
    );

    // 现在看远端自己的 history —— 缺陷 #42 的核心断言。
    // 只看最近几条：注入就发生在刚才，最新的位置。
    let show_history = match detected {
        ShellKind::Bash => "history 5\n",
        _ => "fc -l 5\n",
    };
    writer
        .data_bytes(bytes::Bytes::from(show_history.as_bytes().to_vec()))
        .await
        .expect("写入 history 命令失败");
    let hist = collect(&mut read_half, Duration::from_millis(1500)).await;
    println!("远端 history 输出:\n{hist}");

    assert!(
        !hist.contains("__ssh7_osc7"),
        "钩子进了远端 history（用户执行 history 就能看到）：\n{hist}"
    );
    assert!(
        !hist.contains("stty echo"),
        "恢复回显的命令进了远端 history：\n{hist}"
    );
    assert!(
        !hist.contains("PROMPT_COMMAND"),
        "钩子进了远端 history：\n{hist}"
    );
}

/// 装完钩子之后，工作目录跟踪照常工作，且 history 依旧干净。
#[tokio::test]
#[ignore = "需要真实服务器"]
async fn cwd_tracking_still_works_after_history_cleanup() {
    let host = env_or("SSH7_E2E_HOST", "10.0.80.4");
    let port: u16 = env_or("SSH7_E2E_PORT", "22").parse().unwrap();
    let user = env_or("SSH7_E2E_USER", "root");
    let dir = std::env::var("SSH7_E2E_APP_CONFIG_DIR")
        .map(std::path::PathBuf::from)
        .expect("缺少 SSH7_E2E_APP_CONFIG_DIR");

    let store = SecretStore::new(&dir);
    let account = secrets::password_account(&env_or("SSH7_E2E_HOST_ID", ""));
    let password = store
        .get(&account)
        .expect("读取凭据失败")
        .expect("凭据库里没有这条密码");

    let config = Arc::new(Config {
        inactivity_timeout: Some(Duration::from_secs(60)),
        ..Default::default()
    });
    let handler = ClientHandler {
        host: host.clone(),
        port,
    };
    let mut handle = russh::client::connect(config, (host.as_str(), port), handler)
        .await
        .expect("连接失败");
    handle
        .authenticate_password(&user, password.as_str())
        .await
        .expect("认证失败");

    let channel = handle
        .channel_open_session()
        .await
        .expect("开 channel 失败");
    channel
        .request_pty(true, "xterm-256color", 80, 24, 0, 0, PTY_MODES)
        .await
        .expect("申请 pty 失败");
    channel.request_shell(true).await.expect("申请 shell 失败");
    let (mut read_half, writer) = channel.split();

    let _ = collect(&mut read_half, Duration::from_millis(1500)).await;

    let detect = handle
        .channel_open_session()
        .await
        .expect("开 exec channel 失败");
    detect
        .exec(true, tracker::DETECT_COMMAND)
        .await
        .expect("执行探测命令失败");
    let mut stream = detect.into_stream();
    let mut raw = Vec::new();
    tokio::io::AsyncReadExt::read_to_end(&mut stream, &mut raw)
        .await
        .expect("读取探测结果失败");
    let detected = tracker::parse_detect_output(&String::from_utf8_lossy(&raw));

    let command = tracker::install_command(detected);
    writer
        .data_bytes(bytes::Bytes::from(command.into_bytes()))
        .await
        .expect("写入钩子失败");
    let _ = collect(&mut read_half, Duration::from_millis(1500)).await;

    // 用户敲一条普通命令：它**应该**进 history —— 我们只清理自己那几条。
    writer
        .data_bytes(bytes::Bytes::from("cd /tmp\n"))
        .await
        .expect("写入命令失败");
    let after_cd = collect(&mut read_half, Duration::from_millis(1500)).await;

    assert!(
        after_cd.contains("cd /tmp"),
        "恢复回显之后用户输入应该照常显示：\n{after_cd}"
    );
    assert!(
        after_cd.contains("\x1b]7;") && after_cd.contains("/tmp"),
        "工作目录没有跟上：\n{after_cd}"
    );

    let show_history = match detected {
        ShellKind::Bash => "history 5\n",
        _ => "fc -l 5\n",
    };
    writer
        .data_bytes(bytes::Bytes::from(show_history.as_bytes().to_vec()))
        .await
        .expect("写入 history 命令失败");
    let hist = collect(&mut read_half, Duration::from_millis(1500)).await;
    println!("远端 history 输出:\n{hist}");

    assert!(
        !hist.contains("__ssh7_osc7") && !hist.contains("stty echo"),
        "钩子进了远端 history：\n{hist}"
    );
    assert!(
        hist.contains("cd /tmp"),
        "用户的正常命令必须留在 history 里，不能被误删：\n{hist}"
    );
}
