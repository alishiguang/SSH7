//! 诊断工具：模拟 `storage::keyring::probe()` 的写入 + 删除动作。
//!
//! 设置面板打开时会调用 `keyring::probe()`，它会在钥匙串里写一个
//! `__probe__` 条目再删掉，用来判断系统钥匙串是否可用。
//!
//! 如果这个动作会触发 SecurityAgent 弹窗，那用户在 macOS 上打开设置面板
//! 就会被弹窗骚扰 —— 与本次要解决的问题直接相关，必须实测。
//!
//! 用法：
//!
//! ```sh
//! cargo run --example probetest
//! ```

use std::time::Instant;

fn main() {
    let start = Instant::now();

    let set = keyring::Entry::new("ssh7", "__probe__").map(|e| e.set_password("1").is_ok());
    println!("set   ok={set:?} elapsed={:?}", start.elapsed());

    let del = keyring::Entry::new("ssh7", "__probe__").map(|e| e.delete_credential().is_ok());
    println!("del   ok={del:?} elapsed={:?}", start.elapsed());

    // 再读一次不存在��条目，确认删除干净了。
    let get = keyring::Entry::new("ssh7", "__probe__").map(|e| e.get_password().is_err());
    println!("get   absent={get:?} elapsed={:?}", start.elapsed());
}
