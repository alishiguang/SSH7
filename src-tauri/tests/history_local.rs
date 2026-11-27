//! 「注入钩子不污染远端 history」的本地回归测试。
//!
//! 缺陷 #42：钩子是通过 PTY **输入流**发给远端 shell 的 —— shell 把它当成
//! 用户亲手敲的命令执行，于是这几行进了用户的 history。
//!
//! 这个测试不需要真实服务器：直接在本机的 bash 里跑一遍生产代码生成的
//! 那条安装命令，再用 `history` 看它留没留下痕迹。CI 里也能跑。
//!
//! 真机端到端验证在 `history_clean_e2e.rs`（需要真实服务器）。

use std::io::Write;
use std::process::{Command, Stdio};

use ssh7_lib::ssh::tracker::{self, ShellKind};

/// 在一个干净的 bash 会话里跑一段脚本，返回全部输出。
fn run_in_bash(script: &str) -> String {
    let home = temp_dir("history-local");
    let hist = format!("{home}/.bash_history");

    let mut child = Command::new("bash")
        .args(["--noprofile", "--norc", "-i"])
        .env("HOME", &home)
        .env("HISTFILE", &hist)
        // 用户环境里可能开着忽略规则，会干扰这个测试 —— 明确清掉。
        .env_remove("HISTCONTROL")
        .env_remove("HISTIGNORE")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("起不来 bash");

    child
        .stdin
        .as_mut()
        .expect("拿不到 stdin")
        .write_all(script.as_bytes())
        .expect("写不进 bash");

    let out = child.wait_with_output().expect("等 bash 结束");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// 建一个一次性的目录。
fn temp_dir(tag: &str) -> String {
    let dir = std::env::temp_dir().join(format!(
        "ssh7-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).expect("建不了临时目录");
    dir.to_string_lossy().into_owned()
}

/// bash：装完钩子，history 里不该出现我们的内部命令。
#[test]
fn bash_hook_install_leaves_no_history_trace() {
    let install = tracker::install_command(ShellKind::Bash);
    let script = format!("{install}history\n");

    let out = run_in_bash(&script);

    assert!(!out.contains("__ssh7_osc7"), "钩子进了 history：\n{out}");
    assert!(
        !out.contains("stty echo"),
        "恢复回显的命令进了 history：\n{out}"
    );
    assert!(!out.contains("PROMPT_COMMAND"), "钩子进了 history：\n{out}");
}

/// bash：清理只能删掉自己那几条 —— 用户的命令必须原样留在 history 里。
#[test]
fn bash_hook_install_keeps_user_commands_in_history() {
    let install = tracker::install_command(ShellKind::Bash);
    let script = format!("{install}echo SSH7_USER_MARKER\nhistory\n");

    let out = run_in_bash(&script);

    assert!(
        out.contains("SSH7_USER_MARKER"),
        "用户的命令必须留在 history 里，不能被误删：\n{out}"
    );
    assert!(
        !out.contains("__ssh7_osc7") && !out.contains("stty echo"),
        "钩子进了 history：\n{out}"
    );
}

/// bash：用户自己关掉了 history 时，绝不能把用户上一条命令删掉。
///
/// 这是最危险的情形 —— 命令没被记录，`HISTCMD-1` 就指向用户的真实历史。
/// 宁可留一条我们的痕迹，也不能删掉用户的记录。
#[test]
fn bash_history_cleanup_never_deletes_user_history() {
    let install = tracker::install_command(ShellKind::Bash);
    // 先有一条用户命令，再关掉 history，然后装钩子。
    let script = format!(
        "echo SSH7_PREVIOUS_USER_COMMAND\nset +o history\n{install}set -o history\nhistory\n"
    );

    let out = run_in_bash(&script);

    assert!(
        out.contains("SSH7_PREVIOUS_USER_COMMAND"),
        "用户关掉 history 时，我们的清理不能删掉他的命令：\n{out}"
    );
}

/// zsh：钩子那两行不进 history（zsh 靠忽略空格选项）。
///
/// zsh 没有 `history -d`，所以抹不干净 —— 但能少一条是一条：
/// 「恢复回显」那一行带前导空格，不进 history；只剩「打开忽略选项」那一行。
///
/// zsh 不一定装了 —— 没装就跳过。
#[test]
fn zsh_hook_install_leaves_no_history_trace() {
    if !has_shell("zsh") {
        eprintln!("本机没有 zsh，跳过");
        return;
    }

    let install = tracker::install_command(ShellKind::Zsh);
    let script = format!("{install}history\n");

    let out = run_in_zsh(&script);

    // 恢复回显那一行必须彻底不在 history 里。
    assert!(
        !out.contains("stty echo"),
        "恢复回显的命令进了 zsh 的 history：\n{out}"
    );
    // 钩子本体最多留一条 —— zsh 抹不干净，但不该比这更多。
    let hook_lines = out.lines().filter(|l| l.contains("__ssh7_osc7")).count();
    assert!(
        hook_lines <= 1,
        "zsh 里最多残留一条钩子记录，实际 {hook_lines} 条：\n{out}"
    );
}

/// 在一个干净的 zsh 会话里跑一段脚本，返回全部输出。
fn run_in_zsh(script: &str) -> String {
    let home = temp_dir("history-local-zsh");
    let hist = format!("{home}/.zsh_history");

    let mut child = Command::new("zsh")
        .args(["-f", "-i"])
        .env("HOME", &home)
        .env("HISTFILE", &hist)
        .env_remove("HISTCONTROL")
        .env_remove("HISTIGNORE")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("起不来 zsh");

    child
        .stdin
        .as_mut()
        .expect("拿不到 stdin")
        .write_all(script.as_bytes())
        .expect("写不进 zsh");

    let out = child.wait_with_output().expect("等 zsh 结束");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// 本机有没有这个 shell。
fn has_shell(name: &str) -> bool {
    Command::new(name)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
