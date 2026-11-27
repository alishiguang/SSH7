//! 远端 shell 探测与工作目录上报钩子。
//!
//! 缺陷 #10：旧版无条件向远端写入
//!
//! ```sh
//! export PROMPT_COMMAND='printf "\033]7;file://${HOSTNAME}${PWD}\033\\"'
//! ```
//!
//! 有三个问题：
//!
//! 1. **覆盖用户已有配置** —— `export` 直接赋值，用户原有的 `PROMPT_COMMAND`
//!    被清空。
//! 2. **只对 bash 有效** —— macOS 默认是 zsh，而 zsh 没有 `PROMPT_COMMAND`，
//!    于是在绝大多数 macOS 主机上目录跟踪静默失效。
//! 3. **不追加就重复注入** —— 每次重连都会覆盖一次。
//!
//! 这里改为按 shell 类型注入，并且**只追加不覆盖**。

/// 识别出的远端 shell 类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellKind {
    Bash,
    Zsh,
    Fish,
    /// 无法识别的 shell，不注入钩子。
    Unknown,
}

impl ShellKind {
    /// 供界面展示的名称。
    pub fn label(self) -> &'static str {
        match self {
            ShellKind::Bash => "bash",
            ShellKind::Zsh => "zsh",
            ShellKind::Fish => "fish",
            ShellKind::Unknown => "unknown",
        }
    }
}

/// 根据 `$SHELL` 的路径判断 shell 类型。
pub fn detect_from_path(shell_path: &str) -> ShellKind {
    let name = shell_path
        .rsplit('/')
        .next()
        .unwrap_or(shell_path)
        .trim()
        .to_ascii_lowercase();

    match name.as_str() {
        "bash" => ShellKind::Bash,
        "zsh" => ShellKind::Zsh,
        "fish" => ShellKind::Fish,
        _ => ShellKind::Unknown,
    }
}

/// 生成向远端注入的钩子脚本。
///
/// 返回 `None` 表示该 shell 不支持工作目录跟踪，界面应显示「不可用」
/// 而不是静默显示一个错误的目录。
///
/// **钩子必须是单行**：注入的命令会被 shell 当成一次普通输入执行，
/// 多一行就多打一次提示符（见 [`install_command`]）。
pub fn hook_script(shell: ShellKind) -> Option<&'static str> {
    match shell {
        ShellKind::Bash => Some(BASH_HOOK),
        ShellKind::Zsh => Some(ZSH_HOOK),
        ShellKind::Fish => Some(FISH_HOOK),
        ShellKind::Unknown => None,
    }
}

/// bash：追加到 `PROMPT_COMMAND`，保留用户原有内容。
const BASH_HOOK: &str = concat!(
    "__ssh7_osc7() { printf '\\033]7;file://%s%s\\033\\\\' \"${HOSTNAME:-$(hostname)}\" \"${PWD}\"; }; ",
    "case \";${PROMPT_COMMAND};\" in *\";__ssh7_osc7;\"*) ;; *) ",
    "PROMPT_COMMAND=\"${PROMPT_COMMAND:+${PROMPT_COMMAND};}__ssh7_osc7\" ;; esac"
);

/// zsh：追加到 `precmd_functions` 数组（zsh 没有 `PROMPT_COMMAND`）。
const ZSH_HOOK: &str = concat!(
    "__ssh7_osc7() { printf '\\033]7;file://%s%s\\033\\\\' \"${HOST:-$(hostname)}\" \"${PWD}\"; }; ",
    "typeset -ag precmd_functions; ",
    "[[ -z \"${precmd_functions[(r)__ssh7_osc7]}\" ]] && precmd_functions+=(__ssh7_osc7)"
);

/// fish：通过 `fish_prompt` 事件触发（fish 3.x+）。
const FISH_HOOK: &str = concat!(
    "function __ssh7_osc7 --on-event fish_prompt; ",
    "printf '\\033]7;file://%s%s\\033\\\\' (hostname) $PWD; ",
    "end"
);

/// 什么也不干的占位命令（POSIX 的 `:`）。
///
/// 认不出远端 shell 时用它占第一行，好让两行的结构保持一致。
const NOOP: &str = ":";

/// 擦掉多出来的提示符行，并把回显还回去。
///
/// 钩子是被 shell 「当成一次普通输入」执行的，执行完会**多打一次提示符**。
/// 这里把光标往上挪一行、清到屏幕底，shell 随后重打一次一模一样的提示符 ——
/// 屏幕上看起来跟普通会话完全一样。
///
/// 恢复回显必须**单独一行**：钩子那行万一这个 shell 不认（比如 fish 的
/// 单行 `function` 语法），整行会解析失败 —— 回显是我们关的，得由我们还，
/// 分成两行就互不影响。
const CLEAR_AND_RESTORE: &str = concat!(
    r"printf '\033[1A\r\033[J'; ",
    "stty echo 2>/dev/null || /usr/bin/stty echo 2>/dev/null || /bin/stty echo 2>/dev/null",
    " || printf '\\033[31mSSH7: 无法恢复终端回显，输入不会显示\\033[0m\\n'"
);

/// bash：把刚注入的两行从 history 里抹掉。
///
/// 钩子是写进 PTY **输入流**的 —— shell 把它当成用户亲手敲的命令，于是
/// 这几行进了用户的 history。用户在自己的服务器上执行 `history`，就能看到
/// `__ssh7_osc7`、`stty echo` 这种看不懂的东西（缺陷 #42）。
///
/// **屏幕上看不见 ≠ 没进 history** —— 这是两回事。
///
/// bash 是在**执行前**就把命令行记进 history 的，所以「同一条命令里关掉
/// history」救不了自己。但 `history -d` 能删 —— 它删的正是刚执行的那条。
///
/// 位置不能靠 `HISTCMD` 算：bash 3.2 删掉条目之后计数器会回退，bash 5.x
/// 不会 —— 同一段代码在两个版本上算出来的位置不一样，一个删得到、一个删
/// 不到。所以位置直接问 `history` 要（它报的就是列表位置），两个版本一致。
///
/// 两行得**一起删**，而且先删最后一条：删掉靠前的那条会让后面那条的位置
/// 往前挪一位。
///
/// 下手之前先确认最近两条**确实都是我们的**：一个标记来自钩子本体
/// （`__ssh7_osc7`），另一个来自这段清理代码自己（`__ssh7-clean`）。
/// 万一用户自己关掉了 history，这两行压根没被记录，最近两条就是他自己的
/// 命令 —— 那就不能删。宁可留两条我们的痕迹，也不能删掉用户的记录。
const BASH_HISTORY_CLEANUP: &str = concat!(
    "__ssh7_h=$(history 2); ",
    "case \"$__ssh7_h\" in *'__ssh7_osc7'*'__ssh7-clean'*) ",
    "__ssh7_p1=${__ssh7_h%%$'\\n'*}; __ssh7_p2=${__ssh7_h##*$'\\n'}; ",
    "read -r __ssh7_p1 _ <<< \"$__ssh7_p1\"; read -r __ssh7_p2 _ <<< \"$__ssh7_p2\"; ",
    "history -d \"$__ssh7_p2\" 2>/dev/null; history -d \"$__ssh7_p1\" 2>/dev/null;; esac; ",
    "unset __ssh7_h __ssh7_p1 __ssh7_p2"
);

/// zsh：zsh 没有 `history -d`，但有 `HIST_IGNORE_SPACE` —— 以空格开头的命令
/// 不进 history。这个选项运行时就能开关：开 → 注入（带前导空格）→ 关。
/// 用户的设置原样还回去，不会被改掉。
const ZSH_HISTORY_ON: &str = "setopt HIST_IGNORE_SPACE";
const ZSH_HISTORY_OFF: &str = "unsetopt HIST_IGNORE_SPACE";

/// 组装「静默安装工作目录钩子」要发给远端的那两行命令。
///
/// 远端 PTY 在 `pty-req` 阶段就关掉了回显，所以这两行既不会被内核回显，
/// readline 也不会把它们显示出来 —— 用户屏幕上一点痕迹都不留。
///
/// 第一行是钩子本体（单行命令）；第二行擦掉多出来的提示符并把回显还回去。
/// 即使 shell 不认识（[`ShellKind::Unknown`]），第二行也要发 ——
/// 回显是我们关的，必须由我们还。
///
/// **行数必须保持两行**：多一行就多打一次提示符，屏幕上就会多出一行。
/// history 清理因此是**追加在原有命令后面**，而不是另起一行。
///
/// 至于 history 本身能不能抹干净，因 shell 而异：
///
/// - **bash** —— 完全抹干净（`history -d` 自删）。
/// - **zsh** —— 抹掉两条，剩下「打开忽略选项」那一条短的。
/// - **fish / 认不出来的 shell** —— 没有对应机制，只能留着。
pub fn install_command(shell: ShellKind) -> String {
    match shell {
        ShellKind::Bash => {
            let hook = hook_script(ShellKind::Bash).unwrap_or(NOOP);
            // 清理放在第二行：两行都还在列表里的时候一次删掉。
            format!("{hook}\n{CLEAR_AND_RESTORE}; {BASH_HISTORY_CLEANUP}\n")
        }
        ShellKind::Zsh => {
            let hook = hook_script(ShellKind::Zsh).unwrap_or(NOOP);
            // 第一行开着忽略选项装钩子；第二行带前导空格（于是不进 history），
            // 擦完提示符、还完回显，再把选项关回去。
            format!("{ZSH_HISTORY_ON}; {hook}\n {CLEAR_AND_RESTORE}; {ZSH_HISTORY_OFF}\n")
        }
        ShellKind::Fish => {
            let hook = hook_script(ShellKind::Fish).unwrap_or(NOOP);
            format!("{hook}\n{CLEAR_AND_RESTORE}\n")
        }
        ShellKind::Unknown => format!("{NOOP}\n{CLEAR_AND_RESTORE}\n"),
    }
}

/// 用于探测远端 shell 的命令。
///
/// 在独立的 exec channel 里执行，输出不会出现在交互式终端中。
/// sshd 用用户的登录 shell 承载 exec，因此 `$0` 就是该 shell 的路径。
pub const DETECT_COMMAND: &str = "echo $0";

/// 解析 `$SHELL` 的输出。
pub fn parse_detect_output(raw: &str) -> ShellKind {
    detect_from_path(raw.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_shells_by_path() {
        assert_eq!(detect_from_path("/bin/bash"), ShellKind::Bash);
        assert_eq!(detect_from_path("/bin/zsh"), ShellKind::Zsh);
        assert_eq!(detect_from_path("/usr/local/bin/fish"), ShellKind::Fish);
        assert_eq!(detect_from_path("/bin/sh"), ShellKind::Unknown);
        assert_eq!(detect_from_path(""), ShellKind::Unknown);
    }

    #[test]
    fn detects_shell_case_insensitively() {
        assert_eq!(detect_from_path("/bin/BASH"), ShellKind::Bash);
    }

    #[test]
    fn parses_detect_output() {
        assert_eq!(parse_detect_output("/usr/bin/zsh\n"), ShellKind::Zsh);
        assert_eq!(parse_detect_output("  /bin/bash  "), ShellKind::Bash);
    }

    /// 缺陷 #10 的回归测试：bash 钩子必须是「追加」而不是「覆盖」。
    #[test]
    fn bash_hook_appends_prompt_command() {
        let script = hook_script(ShellKind::Bash).unwrap();
        assert!(
            script.contains("PROMPT_COMMAND=\"${PROMPT_COMMAND:+${PROMPT_COMMAND};}"),
            "bash 钩子必须保留已有的 PROMPT_COMMAND，实际: {script}"
        );
        assert!(
            !script.contains("export PROMPT_COMMAND="),
            "bash 钩子不得用 export 覆盖 PROMPT_COMMAND，实际: {script}"
        );
    }

    #[test]
    fn bash_hook_is_idempotent() {
        let script = hook_script(ShellKind::Bash).unwrap();
        assert!(
            script.contains("case \";${PROMPT_COMMAND};\""),
            "bash 钩子需要做重复注入检测，实际: {script}"
        );
    }

    /// 缺陷 #10 的回归测试：zsh 必须走 precmd_functions。
    #[test]
    fn zsh_hook_uses_precmd_functions() {
        let script = hook_script(ShellKind::Zsh).unwrap();
        assert!(script.contains("precmd_functions"));
        assert!(!script.contains("PROMPT_COMMAND"));
    }

    #[test]
    fn fish_hook_uses_event() {
        let script = hook_script(ShellKind::Fish).unwrap();
        assert!(script.contains("--on-event fish_prompt"));
    }

    #[test]
    fn unknown_shell_has_no_hook() {
        assert!(hook_script(ShellKind::Unknown).is_none());
    }

    /// 所有钩子都必须以 OSC 7 前缀开头，且以 ST 终止。
    #[test]
    fn all_hooks_emit_osc7() {
        for shell in [ShellKind::Bash, ShellKind::Zsh, ShellKind::Fish] {
            let script = hook_script(shell).unwrap();
            assert!(
                script.contains("\\033]7;file://"),
                "{:?} 钩子缺少 OSC7 前缀",
                shell
            );
            assert!(
                script.contains("\\033\\\\"),
                "{:?} 钩子缺少 ST 终止符",
                shell
            );
        }
    }

    /// 缺陷 #21 的回归测试：钩子必须压成一行。
    ///
    /// 注入的命令被 shell 当成一次普通输入执行，每多一行就多打一次提示符，
    /// 屏幕上就会多出一行一行的东西。
    #[test]
    fn hooks_are_single_line() {
        for shell in [ShellKind::Bash, ShellKind::Zsh, ShellKind::Fish] {
            let script = hook_script(shell).unwrap();
            assert!(
                !script.contains('\n'),
                "{:?} 钩子必须压成一行，实际: {script}",
                shell
            );
        }
    }

    /// 缺陷 #21 的回归测试：安装命令必须擦掉提示符、并把回显还回去。
    #[test]
    fn install_command_clears_line_and_restores_echo() {
        let cmd = install_command(ShellKind::Bash);
        assert!(
            cmd.contains(r"printf '\033[1A\r\033[J'"),
            "安装命令必须擦掉多出来的提示符行，实际: {cmd}"
        );
        assert!(
            cmd.contains("stty echo"),
            "安装命令必须恢复回显，实际: {cmd}"
        );
        assert!(
            cmd.contains("__ssh7_osc7"),
            "安装命令必须带上钩子本体，实际: {cmd}"
        );
    }

    /// 恢复回显必须单独一行：钩子那行万一解析失败，回显也得还回去。
    #[test]
    fn install_command_restores_echo_on_its_own_line() {
        for shell in [
            ShellKind::Bash,
            ShellKind::Zsh,
            ShellKind::Fish,
            ShellKind::Unknown,
        ] {
            let cmd = install_command(shell);
            let lines: Vec<&str> = cmd.lines().collect();
            assert_eq!(
                lines.len(),
                2,
                "{shell:?} 的安装命令应该是两行，实际: {cmd}"
            );
            assert!(
                lines[1].contains("stty echo"),
                "{shell:?} 的第二行必须负责恢复回显，实际: {cmd}"
            );
        }
    }

    /// 就算不认得这个 shell，回显也得还回去 —— 是我们关的。
    #[test]
    fn install_command_restores_echo_for_unknown_shell() {
        let cmd = install_command(ShellKind::Unknown);
        assert!(cmd.contains("stty echo"), "实际: {cmd}");
        assert!(!cmd.contains("__ssh7_osc7"), "实际: {cmd}");
    }

    /// 缺陷 #42 的回归测试：bash 的安装命令必须把自己从 history 里抹掉。
    ///
    /// 钩子是写进 PTY 输入流的 —— shell 把它当成用户敲的命令执行，
    /// 于是它进了用户的 history。用户执行 `history` 不该看到这些内部命令。
    #[test]
    fn bash_install_command_cleans_up_its_own_history_entry() {
        let cmd = install_command(ShellKind::Bash);
        assert!(
            cmd.contains("history -d"),
            "bash 必须删掉自己那两条 history，实际: {cmd}"
        );
        assert_eq!(
            cmd.matches("history -d").count(),
            2,
            "两行要一起删，实际: {cmd}"
        );
    }

    /// 位置必须问 `history` 要，不能靠 `HISTCMD` 算。
    ///
    /// bash 3.2 删条目后计数器会回退，bash 5.x 不会 —— 同一段代码在两个
    /// 版本上算出来的位置不一样，一个删得到、一个删不到。
    #[test]
    fn bash_history_cleanup_reads_positions_from_history() {
        let cmd = install_command(ShellKind::Bash);
        assert!(
            !cmd.contains("HISTCMD"),
            "位置不能靠 HISTCMD 算 —— bash 3.2 和 5.x 行为不一样，实际: {cmd}"
        );
        assert!(
            cmd.contains("$(history 2)"),
            "必须从 history 要位置，实际: {cmd}"
        );
    }

    /// 删之前必须先确认最近两条确实是我们的。
    ///
    /// 万一用户自己关掉了 history，这两行压根没被记录 ——
    /// 那最近两条就是他自己的命令，删掉就是数据丢失。
    #[test]
    fn bash_history_cleanup_checks_before_deleting() {
        let cmd = install_command(ShellKind::Bash);
        // 两个标记各归其位：钩子本体的名字 + 这段清理代码自己的标记。
        assert!(
            cmd.contains("*'__ssh7_osc7'*'__ssh7-clean'*)"),
            "确认用的标记必须是两个独一份的，实际: {cmd}"
        );
        // 保护判断和删除动作必须成对出现，顺序是先判断后删除。
        let guard_pos = cmd.find("case \"$__ssh7_h\"").unwrap();
        let del_pos = cmd.find("history -d").unwrap();
        assert!(guard_pos < del_pos, "必须先确认再删除，实际: {cmd}");
    }

    /// 清理必须放在第二行 —— 两行都还在 history 列表里的时候一次删掉。
    ///
    /// 放在第一行的话，钩子那行先被删掉，第二行按旧位置去删就会删错东西。
    #[test]
    fn bash_history_cleanup_runs_on_the_second_line() {
        let cmd = install_command(ShellKind::Bash);
        let lines: Vec<&str> = cmd.lines().collect();
        assert_eq!(lines.len(), 2, "实际: {cmd}");
        assert!(
            !lines[0].contains("history -d"),
            "清理不能放在第一行，实际: {cmd}"
        );
        assert!(
            lines[1].contains("history -d"),
            "清理必须放在第二行，实际: {cmd}"
        );
    }

    /// 缺陷 #42 的回归测试：zsh 没有 `history -d`，改用忽略空格选项。
    ///
    /// 选项运行时就能开关，所以用户的设置原样还回去 —— 不能改了不还。
    #[test]
    fn zsh_install_command_uses_ignore_space_and_restores_it() {
        let cmd = install_command(ShellKind::Zsh);
        assert!(
            cmd.contains("setopt HIST_IGNORE_SPACE"),
            "zsh 必须开忽略空格选项，实际: {cmd}"
        );
        assert!(
            cmd.contains("unsetopt HIST_IGNORE_SPACE"),
            "zsh 必须把选项关回去，不能改了不还，实际: {cmd}"
        );
        // 钩子那一行之后、恢复回显那一行必须带前导空格 —— 靠它躲开 history。
        let lines: Vec<&str> = cmd.lines().collect();
        assert_eq!(lines.len(), 2, "zsh 的安装命令应该是两行，实际: {cmd}");
        assert!(
            lines[1].starts_with(' '),
            "zsh 的第二行必须以空格开头才会被 history 忽略，实际: {cmd}"
        );
        assert!(
            lines[1].contains("unsetopt HIST_IGNORE_SPACE"),
            "zsh 的第二行负责把选项关回去，实际: {cmd}"
        );
    }

    /// 缺陷 #42 的回归测试：fish 没有对应机制，保持原样（两行，不做清理）。
    #[test]
    fn fish_install_command_stays_two_lines_without_cleanup() {
        let cmd = install_command(ShellKind::Fish);
        assert_eq!(cmd.lines().count(), 2, "实际: {cmd}");
        assert!(
            !cmd.contains("history -d"),
            "fish 没有 history -d 可用，实际: {cmd}"
        );
    }

    /// 缺陷 #21 / #42 的共同约束：**行数必须是两行**。
    ///
    /// 多一行就多打一次提示符，屏幕上就会多出一行；history 清理只能
    /// 追加在原有命令后面，不能另起一行。
    #[test]
    fn install_command_is_always_two_lines() {
        for shell in [
            ShellKind::Bash,
            ShellKind::Zsh,
            ShellKind::Fish,
            ShellKind::Unknown,
        ] {
            let cmd = install_command(shell);
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
}
