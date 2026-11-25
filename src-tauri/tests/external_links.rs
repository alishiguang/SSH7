//! 外链权限自检：**点 GitHub 图标要真能打开浏览器**。
//!
//! 缺陷 #40：设置面板底部的 GitHub 图标点了没反应。
//!
//! Tauri 的 opener 插件分**两层**权限，缺一不可：
//!
//! - `opener:allow-open-url` —— 允许**调用** `open_url` 这个命令；
//! - `opener:allow-default-urls` —— 允许**哪些 URL**（scope：`mailto:` / `tel:` /
//!   `http://` / `https://`）。
//!
//! 只给前者，命令能调起来，但 URL 会被 scope 检查拒掉；
//! 前端 catch 之后退化成 `window.open` —— 在 Tauri 的 webview 里那等于没反应，
//! 于是表现为「点了图标毫无动静」，而且**没有任何错误提示**。
//!
//! 权限是**编译期**写进二进制的，源码上看不出来，只有点一下才知道。
//! 所以这里直接读 capability 与生成的 ACL 清单，把这条规则钉死。

use std::path::PathBuf;

/// capability 文件路径（相对 crate 根目录）。
const CAPABILITY_FILE: &str = "capabilities/default.json";

/// 生成的 ACL 清单（`tauri-build` 在编译期产出）。
const ACL_MANIFEST_FILE: &str = "gen/schemas/acl-manifests.json";

/// 仓库地址在前端里的定义处。
const BRIDGE_FILE: &str = "../src/api/bridge.ts";

fn crate_path(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel)
}

fn read_json(rel: &str) -> serde_json::Value {
    let path = crate_path(rel);
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("读不到 {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{} 不是合法 JSON: {e}", path.display()))
}

/// capability 里声明的全部权限。
fn granted_permissions() -> Vec<String> {
    read_json(CAPABILITY_FILE)
        .get("permissions")
        .and_then(|p| p.as_array())
        .map(|list| {
            list.iter()
                .filter_map(|p| p.as_str())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// opener 的 `allow-default-urls` 权限里带哪些 URL 规则。
fn default_url_scope() -> Vec<String> {
    let manifest = read_json(ACL_MANIFEST_FILE);
    manifest
        .get("opener")
        .and_then(|o| o.get("permissions"))
        .and_then(|p| p.get("allow-default-urls"))
        .and_then(|p| p.get("scope"))
        .and_then(|s| s.get("allow"))
        .and_then(|a| a.as_array())
        .map(|list| {
            list.iter()
                .filter_map(|e| e.get("url").and_then(|u| u.as_str()).map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// 前端里写的仓库地址。
fn repo_url() -> String {
    let path = crate_path(BRIDGE_FILE);
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("读不到 {}: {e}", path.display()));
    text.lines()
        .find_map(|line| {
            let line = line.trim();
            line.strip_prefix("export const REPO_URL = '")
                .and_then(|rest| rest.strip_suffix("'"))
                .map(str::to_string)
        })
        .expect("bridge.ts 里找不到 REPO_URL")
}

/// 极简的通配匹配：`https://*` 这样的规则，前缀对上就算命中。
///
/// opener 插件内部用的是 `url::Pattern`，语义与这里一致
/// （`*` 匹配剩余部分），所以这个判断与运行时等价。
fn url_matches(pattern: &str, url: &str) -> bool {
    match pattern.split_once('*') {
        Some((prefix, _)) => url.starts_with(prefix),
        None => pattern == url,
    }
}

/// 命令权限与 scope 权限必须都在。
///
/// 少了 scope，URL 会被拒；少了命令权限，命令根本调不起来。
#[test]
fn opener_command_and_scope_are_both_granted() {
    let perms = granted_permissions();
    assert!(
        perms.iter().any(|p| p == "opener:allow-open-url"),
        "capability 里没有 `opener:allow-open-url`（现有：{perms:?}）—— \
         外链命令调不起来"
    );
    assert!(
        perms.iter().any(|p| p == "opener:allow-default-urls"),
        "capability 里没有 `opener:allow-default-urls`（现有：{perms:?}）—— \
         命令能调但 URL 会被 scope 拒掉，外链点了没反应（缺陷 #40）"
    );
}

/// scope 里必须放行 `https://`。
#[test]
fn scope_allows_https() {
    let scope = default_url_scope();
    assert!(
        scope.iter().any(|p| url_matches(p, "https://example.com")),
        "opener 的默认 scope 里没有 https 规则（现有：{scope:?}）"
    );
}

/// 仓库地址本身必须落在 scope 里。
///
/// 仓库地址是前端写死的常量 —— 万一改成别的协议或域名，这里会红。
#[test]
fn repo_url_is_allowed_by_the_scope() {
    let url = repo_url();
    let scope = default_url_scope();
    assert!(
        scope.iter().any(|p| url_matches(p, &url)),
        "仓库地址 {url} 不在 opener 的 scope 里（现有：{scope:?}）—— \
         图标点了打不开"
    );
}
