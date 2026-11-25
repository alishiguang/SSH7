//! 窗口权限自检：**新开的窗口必须能监听事件**。
//!
//! 缺陷 #37：Tauri 的权限（capability）按窗口标签授权，
//! 之前只授权了 `main`。于是新开的窗口（标签 `host-…`）
//! 拿不到 `event:allow-listen` —— `listen()` 被权限系统拒绝，
//! 异常一路冒泡到终端组件，`attach` 永远不会被调用，
//! 结果就是「提示已连接，终端却一片空白」。
//!
//! 这个坑很隐蔽：权限是**编译期**写进二进制的，源码上完全看不出来，
//! 也不会在开发期报错 —— 只有真的在新窗口里连一次才会暴露。
//!
//! 所以这里直接读 `capabilities/default.json`，
//! 用**与 Tauri 相同的 glob 语义**（`glob::Pattern`）匹配新窗口的标签。

use std::path::PathBuf;

/// 新窗口标签的前缀 —— 见 `commands::window` 里 `host-{uuid}` 的生成。
const HOST_WINDOW_PREFIX: &str = "host-";

/// capability 文件路径（相对 crate 根目录）。
const CAPABILITY_FILE: &str = "capabilities/default.json";

/// 读出所有 capability 声明的窗口模式。
fn authorized_window_patterns() -> Vec<String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(CAPABILITY_FILE);
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("读不到 {}: {e}", path.display()));

    let json: serde_json::Value = serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("{} 不是合法 JSON: {e}", path.display()));

    // capability 文件就是一个对象：{ identifier, windows, permissions, ... }
    json.get("windows")
        .and_then(|w| w.as_array())
        .map(|wins| {
            wins.iter()
                .filter_map(|w| w.as_str())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// 用与 Tauri 相同的 glob 语义判断标签是否被授权。
///
/// Tauri 内部用 `glob::Pattern` 匹配窗口标签
/// （见 `tauri::ipc::authority` 里的 `cmd.windows.iter().any(|w| w.matches(window))`）。
fn pattern_matches(pattern: &str, label: &str) -> bool {
    glob::Pattern::new(pattern)
        .map(|p| p.matches(label))
        .unwrap_or(false)
}

/// 所有 capability 声明过的权限。
fn granted_permissions() -> Vec<String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(CAPABILITY_FILE);
    let text = std::fs::read_to_string(&path).expect("读不到 capability 文件");
    let json: serde_json::Value = serde_json::from_str(&text).expect("capability 不是合法 JSON");

    json.get("permissions")
        .and_then(|p| p.as_array())
        .map(|list| {
            list.iter()
                .filter_map(|p| p.as_str())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// 新窗口（`host-` 前缀）必须被授权，否则终端收不到任何输出。
#[test]
fn newly_opened_windows_are_authorized_for_events() {
    let patterns = authorized_window_patterns();
    assert!(
        !patterns.is_empty(),
        "capability 里一个窗口模式都没有 —— 测试本身失效了"
    );

    // 模拟 `commands::window` 里生成的新窗口标签。
    let sample = format!("{HOST_WINDOW_PREFIX}01a20e5e-098f-74f1-866d-e931d4ae2b4e");

    assert!(
        patterns.iter().any(|p| pattern_matches(p, &sample)),
        "新窗口 {sample} 没有被任何 capability 授权（现有窗口模式：{patterns:?}）—— \
         它的 listen() 会被权限系统拒绝，终端将一片空白（缺陷 #37）"
    );
}

/// 主窗口当然也要被授权。
#[test]
fn main_window_is_authorized_for_events() {
    let patterns = authorized_window_patterns();
    assert!(
        patterns.iter().any(|p| pattern_matches(p, "main")),
        "主窗口没有被授权（现有窗口模式：{patterns:?}）"
    );
}

/// 权限清单里必须包含事件监听权限 —— 没有它终端收不到任何输出。
#[test]
fn event_listen_permission_is_granted() {
    let perms = granted_permissions();
    assert!(
        perms
            .iter()
            .any(|p| p == "core:event:default" || p == "core:event:allow-listen"),
        "capability 里没有事件监听权限（现有：{perms:?}）"
    );
}

/// 缺陷 #39：点左上角红点关不掉窗口。
///
/// Tauri 的规矩是「webview 监听了关闭请求 → 先取消原生关闭，
/// 改由前端接手」，前端必须再调一次 `destroy()` 才算关掉。
/// 而 `destroy()` 要过权限：`core:window:default` 里**只有只读查询**，
/// 没有 `allow-destroy` —— 权限被拒，窗口就永远关不掉。
///
/// 所以这个权限必须显式给。
#[test]
fn window_can_be_closed_by_the_webview() {
    let perms = granted_permissions();
    assert!(
        perms
            .iter()
            .any(|p| p == "core:window:allow-destroy" || p == "core:window:allow-close"),
        "capability 里没有关闭/销毁窗口的权限（现有：{perms:?}）—— \
         前端一旦接手关闭流程就会卡住，窗口关不掉（缺陷 #39）"
    );

    // 这个权限必须**同时**覆盖新窗口 —— 否则新开的窗口照样关不掉。
    let patterns = authorized_window_patterns();
    let sample = format!("{HOST_WINDOW_PREFIX}01a20e5e-098f-74f1-866d-e931d4ae2b4e");
    assert!(
        patterns.iter().any(|p| pattern_matches(p, &sample)),
        "新窗口 {sample} 没有被授权（现有窗口模式：{patterns:?}）—— \
         它的窗口关不掉（缺陷 #39）"
    );
}

/// 缺陷 #39 的另一半：**前端不要劫持原生关闭**。
///
/// 前端注册 `onCloseRequested` 就会让 Tauri 取消原生关闭（`prevent_close`），
/// 把关窗的责任全推给 webview —— 而 webview 马上就要死，
/// 一旦它卡住或权限不够，窗口就再也关不掉。
///
/// 关窗的清理（断开这扇窗的所有 SSH 会话）放在 Rust 侧的 `Destroyed` 事件里，
/// 不依赖前端还活着。
#[test]
fn frontend_does_not_hijack_the_native_close() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../src/App.tsx");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("读不到 {}: {e}", path.display()));

    // 只认「真的调用了这个 API」—— 注释里提到它不算。
    assert!(
        !text.contains(".onCloseRequested("),
        "前端又注册了 onCloseRequested —— 原生关闭会被取消并转交给 webview，\
         窗口可能关不掉（缺陷 #39）。关窗清理请留在 Rust 侧的 Destroyed 事件里。"
    );
}
