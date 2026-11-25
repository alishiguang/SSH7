//! 启动期数据目录迁移。
//!
//! 应用标识符（`identifier`）决定了数据目录的位置：
//! `dirs::config_dir()/<identifier>`。
//!
//! | 平台 | `dirs::config_dir()` | 数据目录 |
//! |------|---------------------|---------|
//! | macOS | `~/Library/Application Support` | `…/Application Support/<identifier>` |
//! | Windows | `%APPDATA%` | `%APPDATA%\<identifier>` |
//! | Linux | `~/.config` | `~/.config/<identifier>` |
//!
//! SSH7 用过的包名不止一个，中间改过几次：
//!
//! - `com.igigi.ssh7` —— 早期开发阶段；
//! - `com.alishiguang.ssh7` —— GitHub 账号还是 `alishiguang` 的时候；
//! - `com.chuaker7.ssh7` —— 现在（跟着 GitHub 账号 `chuaker7`）。
//!
//! 包名一变，数据目录就换一个。**不迁移的话，用户升级后会发现主机列表和
//! 凭据库「凭空消失」**（其实还躺在旧目录里）。
//!
//! ## 为什么是「整个目录搬」而不是「逐个文件拷」
//!
//! 凭据库是 `vault.bin`（密文）+ `vault.key`（密钥）**一对**文件：密钥对得上
//! 才解得开。拆开搬、或者把新旧目录的文件合并到一块儿，都可能把库搞成
//! 「密文在、密钥不配对」的坏状态 —— 那比「找不到文件」还难排查。
//! 所以这里的策略是：**要么整个目录原样搬过去，要么什么都不动。**
//!
//! ## 多个旧目录同时存在怎么办
//!
//! 认领**一个**（清单里靠前的优先），其余原样留在那儿。
//!
//! 不合并是有道理的：早期目录里的主机记录和现在的记录**主机 id 不同**，
//! 合并会变成同台机器出现两行，其中一行还没有对应的密码 —— 比不合并更糟。
//! 留在旧目录里的那份不碍事，应用压根不会去读它。

use std::path::{Path, PathBuf};

/// 旧的应用标识符清单，**从新到旧**排列。
///
/// 排在前面的是「更接近当前版本」的旧包名 —— 多个旧目录同时存在时，
/// 优先认领数据更全的那一个。
pub const LEGACY_IDENTIFIERS: &[&str] = &["com.alishiguang.ssh7", "com.igigi.ssh7"];

/// 迁移结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Migration {
    /// 没有旧目录，没什么可搬的（全新安装）。
    NothingToDo,
    /// 旧目录整体搬到了新目录。
    Moved {
        /// 搬的是哪个旧包名的目录。
        from: &'static str,
    },
    /// 新目录里已经有数据了 —— 不动它。用户已经用新标识符的版本存过东西了，
    /// 这时候覆盖会丢数据。
    Skipped,
}

/// 列出所有存在的旧标识符目录，按清单顺序（从新到旧）。
pub fn legacy_dirs_for(new_dir: &Path) -> Vec<(&'static str, PathBuf)> {
    let Some(parent) = new_dir.parent() else {
        return Vec::new();
    };
    LEGACY_IDENTIFIERS
        .iter()
        .filter_map(|identifier| {
            let dir = parent.join(identifier);
            // 理论上不会撞上自己，但真撞上了就别去搬自己。
            if dir == new_dir || !dir.is_dir() {
                None
            } else {
                Some((*identifier, dir))
            }
        })
        .collect()
}

/// 把旧标识符目录下的数据搬到新目录。
///
/// 幂等：搬过之后旧目录就不存在了，再跑一次什么都不做。
pub fn migrate_legacy_config_dir(new_dir: &Path) -> std::io::Result<Migration> {
    let candidates = legacy_dirs_for(new_dir);
    if candidates.is_empty() {
        return Ok(Migration::NothingToDo);
    }
    // 新目录已经有内容 → 用户已经用新标识符的版本存过数据，不能动。
    if dir_has_entries(new_dir) {
        return Ok(Migration::Skipped);
    }
    if let Some(parent) = new_dir.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // 新目录存在但为空 —— 先让位，否则 rename 会因为目标已存在而失败。
    if new_dir.exists() {
        std::fs::remove_dir(new_dir)?;
    }

    // 只认领一个：清单里靠前的优先（数据更全的那份）。
    let (identifier, legacy_dir) = &candidates[0];
    match std::fs::rename(legacy_dir, new_dir) {
        Ok(()) => Ok(Migration::Moved { from: identifier }),
        Err(rename_err) => {
            // 同一个父目录下的 rename 基本不会失败。真失败了就退回「逐个文件拷」，
            // 拷完把旧目录删掉 —— 免得磁盘上留两份凭据。
            copy_dir(legacy_dir, new_dir)?;
            std::fs::remove_dir_all(legacy_dir)?;
            tracing::warn!("数据目录改名失败，已改为拷贝方式迁移：{rename_err}");
            Ok(Migration::Moved { from: identifier })
        }
    }
}

/// 目录里有没有东西。
fn dir_has_entries(dir: &Path) -> bool {
    match std::fs::read_dir(dir) {
        Ok(mut entries) => entries.next().is_some(),
        // 目录不存在或读不了 —— 当作「没有东西」。
        Err(_) => false,
    }
}

/// 递归拷贝目录。
///
/// `std::fs::copy` 会保留源文件的权限位 —— 这点很关键：
/// `vault.key` 是 `0600`，拷过去必须还是 `0600`。
fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "ssh7-legacy-dir-test-{tag}-{}",
            uuid::Uuid::now_v7()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn new_dir_of(root: &Path) -> PathBuf {
        root.join("com.chuaker7.ssh7")
    }

    fn legacy_dir_of(root: &Path, identifier: &str) -> PathBuf {
        root.join(identifier)
    }

    #[test]
    fn moves_legacy_dir_when_new_dir_is_absent() {
        let root = temp_root("absent");
        let new_dir = new_dir_of(&root);
        let legacy_dir = legacy_dir_of(&root, "com.alishiguang.ssh7");

        std::fs::create_dir_all(&legacy_dir).unwrap();
        std::fs::write(legacy_dir.join("hosts.json"), b"{\"hosts\":[]}").unwrap();
        std::fs::write(legacy_dir.join("vault.bin"), b"cipher").unwrap();
        std::fs::write(legacy_dir.join("vault.key"), b"0123456789abcdef").unwrap();

        assert_eq!(
            migrate_legacy_config_dir(&new_dir).unwrap(),
            Migration::Moved {
                from: "com.alishiguang.ssh7"
            }
        );

        assert!(!legacy_dir.exists(), "旧目录应该被搬空了");
        assert!(new_dir.join("hosts.json").exists());
        assert!(new_dir.join("vault.bin").exists());
        assert!(new_dir.join("vault.key").exists());

        std::fs::remove_dir_all(&root).ok();
    }

    /// 早期那个包名（`com.igigi.ssh7`）也要能认出来。
    #[test]
    fn moves_early_build_package_name() {
        let root = temp_root("igigi");
        let new_dir = new_dir_of(&root);
        let legacy_dir = legacy_dir_of(&root, "com.igigi.ssh7");

        std::fs::create_dir_all(&legacy_dir).unwrap();
        std::fs::write(legacy_dir.join("hosts.json"), b"{}").unwrap();

        assert_eq!(
            migrate_legacy_config_dir(&new_dir).unwrap(),
            Migration::Moved {
                from: "com.igigi.ssh7"
            }
        );
        assert!(new_dir.join("hosts.json").exists());

        std::fs::remove_dir_all(&root).ok();
    }

    /// 两个旧目录同时存在 —— 认领更新的那个，更旧的那个原样留着。
    ///
    /// 旧目录里的主机 id 和现在的不一样，合并会变成同一台机器两行，
    /// 其中一行还没密码 —— 比不合并更糟。
    #[test]
    fn claims_the_newer_legacy_dir_and_leaves_the_older_one() {
        let root = temp_root("both");
        let new_dir = new_dir_of(&root);
        let newer = legacy_dir_of(&root, "com.alishiguang.ssh7");
        let older = legacy_dir_of(&root, "com.igigi.ssh7");

        std::fs::create_dir_all(&newer).unwrap();
        std::fs::write(newer.join("hosts.json"), b"NEWER").unwrap();
        std::fs::create_dir_all(&older).unwrap();
        std::fs::write(older.join("hosts.json"), b"OLDER").unwrap();

        assert_eq!(
            migrate_legacy_config_dir(&new_dir).unwrap(),
            Migration::Moved {
                from: "com.alishiguang.ssh7"
            }
        );
        assert_eq!(
            std::fs::read_to_string(new_dir.join("hosts.json")).unwrap(),
            "NEWER"
        );
        assert!(older.exists(), "更旧的旧目录应该原样留着，不能合并也不能删");

        std::fs::remove_dir_all(&root).ok();
    }

    /// 新目录已存在但是空的 —— 也得搬。
    #[test]
    fn moves_legacy_dir_when_new_dir_exists_but_is_empty() {
        let root = temp_root("empty");
        let new_dir = new_dir_of(&root);
        let legacy_dir = legacy_dir_of(&root, "com.alishiguang.ssh7");

        std::fs::create_dir_all(&legacy_dir).unwrap();
        std::fs::write(legacy_dir.join("hosts.json"), b"{}").unwrap();
        std::fs::create_dir_all(&new_dir).unwrap();

        assert_eq!(
            migrate_legacy_config_dir(&new_dir).unwrap(),
            Migration::Moved {
                from: "com.alishiguang.ssh7"
            }
        );
        assert!(new_dir.join("hosts.json").exists());

        std::fs::remove_dir_all(&root).ok();
    }

    /// 新目录里已经有数据 —— 一个字节都不能碰。
    #[test]
    fn keeps_new_dir_when_it_already_has_data() {
        let root = temp_root("hasdata");
        let new_dir = new_dir_of(&root);
        let legacy_dir = legacy_dir_of(&root, "com.alishiguang.ssh7");

        std::fs::create_dir_all(&legacy_dir).unwrap();
        std::fs::write(legacy_dir.join("hosts.json"), b"OLD").unwrap();
        std::fs::create_dir_all(&new_dir).unwrap();
        std::fs::write(new_dir.join("hosts.json"), b"NEW").unwrap();

        assert_eq!(
            migrate_legacy_config_dir(&new_dir).unwrap(),
            Migration::Skipped
        );
        assert_eq!(
            std::fs::read_to_string(new_dir.join("hosts.json")).unwrap(),
            "NEW"
        );
        assert!(legacy_dir.exists(), "旧目录应该原样留着");

        std::fs::remove_dir_all(&root).ok();
    }

    /// 全新安装 —— 没有旧目录，什么都不做。
    #[test]
    fn does_nothing_when_legacy_dir_absent() {
        let root = temp_root("fresh");
        let new_dir = new_dir_of(&root);

        assert_eq!(
            migrate_legacy_config_dir(&new_dir).unwrap(),
            Migration::NothingToDo
        );
        assert!(!new_dir.exists(), "不该凭空造出目录");

        std::fs::remove_dir_all(&root).ok();
    }

    /// 幂等：搬过一次之后再跑，不该出事。
    ///
    /// 搬完之后旧目录已经不存在了，所以第二次是「没东西可搬」——
    /// 不会重复搬、也不会报错。
    #[test]
    fn is_idempotent() {
        let root = temp_root("idempotent");
        let new_dir = new_dir_of(&root);
        let legacy_dir = legacy_dir_of(&root, "com.alishiguang.ssh7");

        std::fs::create_dir_all(&legacy_dir).unwrap();
        std::fs::write(legacy_dir.join("hosts.json"), b"{}").unwrap();

        assert_eq!(
            migrate_legacy_config_dir(&new_dir).unwrap(),
            Migration::Moved {
                from: "com.alishiguang.ssh7"
            }
        );
        assert_eq!(
            migrate_legacy_config_dir(&new_dir).unwrap(),
            Migration::NothingToDo
        );
        assert!(new_dir.join("hosts.json").exists());

        std::fs::remove_dir_all(&root).ok();
    }

    /// 凭据库是一对文件，搬过去必须都还在。
    #[test]
    fn keeps_vault_pair_together() {
        let root = temp_root("vault");
        let new_dir = new_dir_of(&root);
        let legacy_dir = legacy_dir_of(&root, "com.alishiguang.ssh7");

        std::fs::create_dir_all(&legacy_dir).unwrap();
        std::fs::write(legacy_dir.join("vault.bin"), b"ciphertext").unwrap();
        std::fs::write(legacy_dir.join("vault.key"), b"0123456789abcdef").unwrap();

        migrate_legacy_config_dir(&new_dir).unwrap();

        assert_eq!(
            std::fs::read_to_string(new_dir.join("vault.bin")).unwrap(),
            "ciphertext"
        );
        assert_eq!(
            std::fs::read_to_string(new_dir.join("vault.key")).unwrap(),
            "0123456789abcdef"
        );

        std::fs::remove_dir_all(&root).ok();
    }
}
