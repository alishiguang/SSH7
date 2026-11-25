//! 「静默安装 OSC 7 钩子」的真实端到端测试。
//!
//! 缺陷 #21：以前每次开 session，终端里都会冒出来几行 shell 代码 ——
//! 那是钩子脚本被写进 PTY 输入流后，内核回显了一遍、bash 的 readline
//! 启动时又重绘了一遍。
//!
//! 现在改成：`pty-req` 阶段就让远端 PTY 关掉回显，注入就完全看不见；
//! 装完再用 `stty echo` 把回显还回去。这个测试直接用**生产代码里那几条
//! 字符串**（`russh::Pty::ECHO = 0` + `tracker::install_command`），
//! 到真服务器上验证三件事：
//!
//! 1. 钩子代码一个字都不出现在终端输出里；
//! 2. 提示符只出现一次（没有多出来的空提示符行）；
//! 3. 回显恢复之后，用户输入照常显示，且工作目录跟踪照常工作。
//!
//! 需要真实服务器时显式运行：
//!
//! ```sh
//! cd src-tauri
//! SSH7_E2E_HOST=10.0.80.4 SSH7_E2E_PORT=22 SSH7_E2E_USER=root \
//! SSH7_E2E_APP_CONFIG_DIR="$HOME/Library/Application Support/com.chuaker7.ssh7" \
//! cargo test --test osc7_quiet_e2e -- --ignored --nocapture --test-threads=1
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

/// 钩子安装后，屏幕上不该留下任何痕迹，功能却要照常工作。
#[tokio::test]
#[ignore = "需要真实服务器"]
async fn hook_install_leaves_no_trace_on_screen() {
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
    // 再按探测结果装钩子。这里连顺序都跟 `ssh::client::connect` 一致。
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

    // 生产代码用的那两行命令
    let command = tracker::install_command(detected);
    writer
        .data_bytes(bytes::Bytes::from(command.into_bytes()))
        .await
        .expect("写入钩子失败");
    let after = collect(&mut read_half, Duration::from_millis(1500)).await;

    assert!(
        !after.contains("__ssh7_osc7"),
        "钩子代码出现在了屏幕上：\n{after}"
    );
    assert!(
        !after.contains("PROMPT_COMMAND"),
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

    // 提示符：擦除之前一个、之后一个 —— 中间那个被擦掉了，
    // 屏幕上最终只剩一行提示符，跟普通会话一模一样。
    let first_prompt = after.find(":~#").expect("应该看到提示符");
    let erase = after.find("\x1b[J").expect("应该看到擦除序列");
    let last_prompt = after.rfind(":~#").expect("应该看到提示符");
    assert!(
        first_prompt < erase,
        "擦除应该发生在多出来的那个提示符之后：\n{after}"
    );
    assert!(
        erase < last_prompt,
        "擦除应该发生在最后一个提示符之前：\n{after}"
    );

    // 模拟用户输入：看得见，且工作目录跟得上。
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
}

/// 安装命令必须是两行 —— 钩子一行、恢复回显一行。
#[test]
fn install_command_is_two_lines() {
    for shell in [
        ShellKind::Bash,
        ShellKind::Zsh,
        ShellKind::Fish,
        ShellKind::Unknown,
    ] {
        let cmd = tracker::install_command(shell);
        assert_eq!(
            cmd.lines().count(),
            2,
            "{shell:?} 的安装命令应该是两行，实际: {cmd}"
        );
        assert!(
            cmd.lines().nth(1).unwrap_or_default().contains("stty echo"),
            "{shell:?} 的第二行必须负责恢复回显，实际: {cmd}"
        );
    }
}
