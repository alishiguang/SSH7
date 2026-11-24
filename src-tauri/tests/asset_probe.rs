//! 产物自检：确认**打出来的包**里带着关键修复。
//!
//! 这类问题有个讨厌的地方：源码明明改了，本地也编译通过，
//! 但真正发出去的安装包还是坏的（v1.8.0 就在 GitHub 上放过一版
//! 「连上了却进不去服务器」的坏包）。
//!
//! 所以这里不看源码，**直接问编译期内嵌进二进制的资源**：
//! `generate_context!()` 会把 `dist/` 压进二进制，测试再把它取出来验。

use tauri::utils::assets::AssetKey;

/// 事件定向投递的修复必须在内嵌前端里。
///
/// 缺陷 #35：JS 的 `listen()` 默认注册成 `EventTarget::Any`，
/// 而定向事件匹配不到 `Any` —— 事件会被静默丢弃，
/// 表现为「已连接」但终端空白、输入不回显。
/// 修复是监听时带上本窗口标签，产物里体现为
/// `plugin:event|listen",{event:...,target:...}`。
#[test]
fn embedded_frontend_carries_event_target_fix() {
    let ctx: tauri::Context<tauri::Wry> = tauri::generate_context!();

    // 资源名带内容哈希，所以不写死文件名，改成把每个 .js 都取出来看。
    let mut checked = 0usize;
    let mut found_listen = false;
    let mut found_target = false;

    for (path, _) in ctx.assets().iter() {
        if !path.ends_with(".js") {
            continue;
        }
        let key = AssetKey::from(path.as_ref());
        let Some(bytes) = ctx.assets().get(&key) else {
            continue;
        };
        checked += 1;
        let has_listen = bytes
            .windows(b"plugin:event|listen".len())
            .any(|w| w == b"plugin:event|listen");
        let has_target = bytes.windows(b"target:".len()).any(|w| w == b"target:");
        println!(
            "{path}: {} 字节, plugin:event|listen={has_listen} target:={has_target}",
            bytes.len()
        );
        found_listen |= has_listen;
        found_target |= has_target;
    }

    assert!(checked > 0, "一个内嵌 js 都没取到");
    assert!(found_listen, "内嵌 JS 里没有 plugin:event|listen");
    assert!(found_target, "内嵌 JS 里没有 target: —— 定向投递修复没生效");
}
