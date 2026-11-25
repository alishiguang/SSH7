//! 身份自检：**打出来的包**用的是不是当前这套包名。
//!
//! GitHub 账号改名之后，应用标识符跟着从 `com.alishiguang.ssh7`
//! 换成了 `com.chuaker7.ssh7`。标识符不是装饰 —— 它决定了
//! **数据目录的位置**（`dirs::config_dir()/<identifier>`），
//! 换错了用户的主机列表和凭据库就会「凭空消失」。
//!
//! 标识符是**编译期**写进二进制的，源码上看不出来。
//! 所以这里不问源码，直接问编译期内嵌进二进制的上下文。
//!
//! 另外还钉住「前端写死的仓库地址」和包名是同一套 ——
//! 改名改一半是最容易漏的。

use std::path::PathBuf;

/// 当前的应用标识符。
const EXPECTED_IDENTIFIER: &str = "com.chuaker7.ssh7";

/// 当前的 GitHub 账号。
const EXPECTED_OWNER: &str = "chuaker7";

/// 前端里仓库地址的定义处。
const BRIDGE_FILE: &str = "../src/api/bridge.ts";

fn crate_path(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel)
}

/// 内嵌上下文里的标识符必须就是当前包名。
///
/// 这条直接读 `generate_context!()` —— 也就是**真的被编进二进制的那份**。
#[test]
fn embedded_context_uses_the_current_identifier() {
    let ctx: tauri::Context<tauri::Wry> = tauri::generate_context!();
    assert_eq!(
        ctx.config().identifier,
        EXPECTED_IDENTIFIER,
        "内嵌上下文里的标识符不是 {EXPECTED_IDENTIFIER} —— \
         数据目录会指向别的地方，用户的主机列表和凭据库会「消失」"
    );
}

/// 前端写死的仓库地址必须是当前账号下的那个仓库。
#[test]
fn repo_url_points_at_the_current_owner() {
    let path = crate_path(BRIDGE_FILE);
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("读不到 {}: {e}", path.display()));
    let url = text
        .lines()
        .find_map(|line| {
            let line = line.trim();
            line.strip_prefix("export const REPO_URL = '")
                .and_then(|rest| rest.strip_suffix("'"))
                .map(str::to_string)
        })
        .expect("bridge.ts 里找不到 REPO_URL");

    assert!(
        url == format!("https://github.com/{EXPECTED_OWNER}/SSH7"),
        "仓库地址是 {url}，应该是 https://github.com/{EXPECTED_OWNER}/SSH7"
    );
}

/// 旧的包名只能出现在「迁移清单」里，不能出现在标识符上。
///
/// 留着旧包名是为了把用户的数据目录搬过来，不是为了继续用旧身份。
#[test]
fn old_package_name_is_only_used_for_migration() {
    let path = crate_path("src/storage/legacy_dir.rs");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("读不到 {}: {e}", path.display()));

    // 迁移清单里必须仍然认得旧包名 —— 否则老用户的数据就搬不过来了。
    assert!(
        text.contains("com.alishiguang.ssh7"),
        "迁移清单里没有旧包名 com.alishiguang.ssh7 —— 老用户的数据搬不过来"
    );

    // 但标识符本身必须是新的。
    let ctx: tauri::Context<tauri::Wry> = tauri::generate_context!();
    assert_ne!(
        ctx.config().identifier,
        "com.alishiguang.ssh7",
        "标识符还是旧的 —— 数据目录会跟旧版本撞在一起"
    );
}
