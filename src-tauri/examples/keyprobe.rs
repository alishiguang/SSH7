//! 诊断工具：读取系统钥匙串里的一个条目。
//!
//! macOS 上未签名的应用读取钥匙串条目时可能弹窗索要「登录」钥匙串密码，
//! 这时本程序会**卡住**（弹窗在等着）。所以它能用来判断：
//! 「某个二进制到底能不能读到这个条目」。
//!
//! 用法：
//!
//! ```sh
//! cargo run --example keyprobe -- <service> <account>
//! ```
//!
//! 只打印秘密的**长度**，不会把内容打到终端上。

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let service = args.get(1).cloned().unwrap_or_default();
    let account = args.get(2).cloned().unwrap_or_default();

    match keyring::Entry::new(&service, &account) {
        Ok(entry) => match entry.get_password() {
            Ok(secret) => println!("OK len={}", secret.len()),
            Err(e) => println!("ERR {e}"),
        },
        Err(e) => println!("ERR entry {e}"),
    }
}
