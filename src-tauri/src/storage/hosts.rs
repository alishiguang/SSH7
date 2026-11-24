//! 已保存主机的本地存储。
//!
//! 安全约束（缺陷 #1）：**本文件永远不包含密码或私钥口令**。
//! 秘密只保存在凭据库里，`hosts.json` 仅保存「是否已保存秘密」
//! 这样的布尔标记，用于界面提示与凭据库条目的清理。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};

pub use crate::types::AuthType;

/// 一台已保存的主机。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedHost {
    pub id: String,
    pub label: String,
    /// 所属分组。留空表示不分组，界面里排在最前面。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_type: AuthType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private_key_path: Option<String>,
    /// 凭据库中是否存在该主机的密码。
    #[serde(default)]
    pub has_password: bool,
    /// 凭据库中是否存在该主机的私钥口令。
    #[serde(default)]
    pub has_passphrase: bool,
}

/// 主机列表文件。带版本号，便于将来迁移。
#[derive(Debug, Clone, Serialize, Deserialize)]
struct HostsFile {
    version: u32,
    #[serde(default)]
    hosts: Vec<SavedHost>,
    /// 分组清单与显示顺序。**可以包含空分组** —— 用户先把组建好，
    /// 往里放机器时直接选。旧文件没有这一项时按空处理，
    /// 分组仍能从主机上的 `group` 字段推出来。
    #[serde(default)]
    groups: Vec<String>,
}

/// `hosts.json` 的读写器。
#[derive(Debug, Clone)]
pub struct HostsStore {
    path: PathBuf,
}

impl HostsStore {
    pub fn new(dir: &Path) -> Self {
        Self {
            path: dir.join("hosts.json"),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 读取全部主机。文件不存在时返回空列表，不视为错误。
    pub fn load(&self) -> AppResult<Vec<SavedHost>> {
        Ok(self.load_file()?.hosts)
    }

    /// 读取分组清单（名字与顺序）。
    pub fn groups(&self) -> AppResult<Vec<String>> {
        Ok(self.load_file()?.groups)
    }

    /// 读整个文件。
    fn load_file(&self) -> AppResult<HostsFile> {
        if !self.path.exists() {
            return Ok(HostsFile {
                version: 1,
                hosts: Vec::new(),
                groups: Vec::new(),
            });
        }
        let raw = std::fs::read_to_string(&self.path)?;
        if raw.trim().is_empty() {
            return Ok(HostsFile {
                version: 1,
                hosts: Vec::new(),
                groups: Vec::new(),
            });
        }
        match serde_json::from_str::<HostsFile>(&raw) {
            Ok(f) => Ok(f),
            Err(e) => {
                // 文件损坏时不让整个应用起不来：备份后从空列表开始。
                let backup = self.path.with_extension("json.corrupt");
                let _ = std::fs::rename(&self.path, &backup);
                tracing::warn!(error = %e, "hosts.json 解析失败，已备份并重置");
                Ok(HostsFile {
                    version: 1,
                    hosts: Vec::new(),
                    groups: Vec::new(),
                })
            }
        }
    }

    /// 原子写入：先写临时文件再重命名，避免中途崩溃留下半截 JSON。
    pub fn save(&self, hosts: &[SavedHost]) -> AppResult<()> {
        // 保留原有的分组清单，别被一次主机改动冲掉。
        let groups = self.load_file()?.groups;
        self.save_all(hosts, &groups)
    }

    /// 连同分组清单一起写入。
    fn save_all(&self, hosts: &[SavedHost], groups: &[String]) -> AppResult<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = HostsFile {
            version: 1,
            hosts: hosts.to_vec(),
            groups: groups.to_vec(),
        };
        let body = serde_json::to_string_pretty(&file)
            .map_err(|e| AppError::Internal(format!("序列化主机列表失败: {e}")))?;

        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, body)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }

    /// 新增或更新（按 id 匹配）。
    ///
    /// 顺带把主机所属的分组登记进清单 —— 这样分组的位置是稳定的，
    /// 之后再往里加机器不会把整组挤到后面去。
    pub fn upsert(&self, host: &SavedHost) -> AppResult<(Vec<SavedHost>, Vec<String>)> {
        let mut file = self.load_file()?;
        match file.hosts.iter_mut().find(|h| h.id == host.id) {
            Some(existing) => *existing = host.clone(),
            None => file.hosts.push(host.clone()),
        }
        if let Some(group) = host.group.as_deref().map(str::trim) {
            if !group.is_empty() && !file.groups.iter().any(|g| g == group) {
                file.groups.push(group.to_string());
            }
        }
        self.save_all(&file.hosts, &file.groups)?;
        Ok((file.hosts, file.groups))
    }

    /// 按 id 删除。分组清单不受影响。
    pub fn remove(&self, id: &str) -> AppResult<(Vec<SavedHost>, Vec<String>)> {
        let mut file = self.load_file()?;
        file.hosts.retain(|h| h.id != id);
        self.save_all(&file.hosts, &file.groups)?;
        Ok((file.hosts, file.groups))
    }

    /// 新建一个分组。名字重复时什么都不做。
    pub fn create_group(&self, name: &str) -> AppResult<(Vec<SavedHost>, Vec<String>)> {
        let name = name.trim();
        let mut file = self.load_file()?;
        if !name.is_empty() && !file.groups.iter().any(|g| g == name) {
            file.groups.push(name.to_string());
        }
        self.save_all(&file.hosts, &file.groups)?;
        Ok((file.hosts, file.groups))
    }

    /// 重命名分组：组里的主机一起改到新名字，清单里的位置也换掉。
    ///
    /// 新名字跟已有分组重名时两组自然合并 —— 这正是用户想要的效果，
    /// 所以这里不做「重名报错」。
    pub fn rename_group(
        &self,
        old_name: &str,
        new_name: &str,
    ) -> AppResult<(Vec<SavedHost>, Vec<String>)> {
        let old_name = old_name.trim();
        let new_name = new_name.trim();
        let mut file = self.load_file()?;

        for h in file.hosts.iter_mut() {
            if h.group.as_deref().map(str::trim) == Some(old_name) {
                h.group = Some(new_name.to_string());
            }
        }

        // 清单里换名：已经有同名分组就不重复登记。
        if let Some(pos) = file.groups.iter().position(|g| g == old_name) {
            if file.groups.iter().any(|g| g == new_name) {
                file.groups.remove(pos);
            } else {
                file.groups[pos] = new_name.to_string();
            }
        }
        self.save_all(&file.hosts, &file.groups)?;
        Ok((file.hosts, file.groups))
    }

    /// 删除分组：组里的主机回到「未分组」，**主机本身不删**。
    pub fn remove_group(&self, name: &str) -> AppResult<(Vec<SavedHost>, Vec<String>)> {
        let name = name.trim();
        let mut file = self.load_file()?;

        for h in file.hosts.iter_mut() {
            if h.group.as_deref().map(str::trim) == Some(name) {
                h.group = None;
            }
        }
        file.groups.retain(|g| g.trim() != name);

        self.save_all(&file.hosts, &file.groups)?;
        Ok((file.hosts, file.groups))
    }

    /// 重排分组顺序。
    ///
    /// `order` 是新的顺序（只写分组名）。清单里没提到的分组保持相对顺序接在后面 ——
    /// 这样前端漏传一个也不会把分组弄丢。
    pub fn reorder_groups(&self, order: &[String]) -> AppResult<(Vec<SavedHost>, Vec<String>)> {
        let file = self.load_file()?;
        let mut next: Vec<String> = Vec::new();
        for name in order {
            let name = name.trim();
            if name.is_empty() {
                continue;
            }
            // 认识这个名字才收：清单里的，或主机上正用着的。
            let known = file.groups.iter().any(|g| g == name)
                || file
                    .hosts
                    .iter()
                    .any(|h| h.group.as_deref().map(str::trim) == Some(name));
            if known && !next.iter().any(|g| g == name) {
                next.push(name.to_string());
            }
        }
        for g in &file.groups {
            if !next.iter().any(|x| x == g) {
                next.push(g.clone());
            }
        }
        self.save_all(&file.hosts, &next)?;
        Ok((file.hosts, next))
    }

    /// 把主机挪到某个分组。`group` 为空表示回到「未分组」。
    ///
    /// 目标分组还不存在就顺手登记进清单 —— 拖到哪，哪就有了。
    pub fn set_group(
        &self,
        host_id: &str,
        group: Option<&str>,
    ) -> AppResult<(Vec<SavedHost>, Vec<String>)> {
        let mut file = self.load_file()?;
        let name = group.map(str::trim).filter(|s| !s.is_empty());

        let mut found = false;
        for h in file.hosts.iter_mut() {
            if h.id == host_id {
                h.group = name.map(str::to_string);
                found = true;
            }
        }
        if !found {
            return Err(AppError::NoSuchHost("主机不存在".into()));
        }
        if let Some(n) = name {
            if !file.groups.iter().any(|g| g == n) {
                file.groups.push(n.to_string());
            }
        }

        self.save_all(&file.hosts, &file.groups)?;
        Ok((file.hosts, file.groups))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    fn temp_dir() -> PathBuf {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let p = std::env::temp_dir().join(format!("ssh7-hosts-test-{n}"));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn sample(id: &str) -> SavedHost {
        SavedHost {
            id: id.into(),
            label: "test".into(),
            group: None,
            host: "127.0.0.1".into(),
            port: 22,
            username: "root".into(),
            auth_type: AuthType::Password,
            private_key_path: None,
            has_password: true,
            has_passphrase: false,
        }
    }

    #[test]
    fn load_missing_file_returns_empty() {
        let store = HostsStore::new(&temp_dir());
        assert!(store.load().unwrap().is_empty());
    }

    #[test]
    fn upsert_and_remove() {
        let store = HostsStore::new(&temp_dir());
        store.upsert(&sample("a")).unwrap();
        let hosts = store.load().unwrap();
        assert_eq!(hosts.len(), 1);

        store.upsert(&sample("b")).unwrap();
        assert_eq!(store.load().unwrap().len(), 2);

        store.remove("a").unwrap();
        assert_eq!(store.load().unwrap().len(), 1);
    }

    #[test]
    fn upsert_replaces_same_id() {
        let store = HostsStore::new(&temp_dir());
        store.upsert(&sample("a")).unwrap();
        let mut updated = sample("a");
        updated.label = "changed".into();
        store.upsert(&updated).unwrap();
        let hosts = store.load().unwrap();
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].label, "changed");
    }

    /// 分组要能原样存回来，没分组的旧记录也不能因此读不出来。
    #[test]
    fn group_round_trips() {
        let dir = temp_dir();
        let store = HostsStore::new(&dir);

        let mut grouped = sample("a");
        grouped.group = Some("生产环境".into());
        store.upsert(&grouped).unwrap();

        let mut plain = sample("b");
        plain.group = None;
        store.upsert(&plain).unwrap();

        let hosts = store.load().unwrap();
        assert_eq!(hosts.len(), 2);
        assert_eq!(hosts[0].group.as_deref(), Some("生产环境"));
        assert_eq!(hosts[1].group, None);

        // 没分组的主机不往 hosts.json 里塞空字段。
        let raw = std::fs::read_to_string(dir.join("hosts.json")).unwrap();
        let json: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let entries = json.get("hosts").and_then(|h| h.as_array()).unwrap();
        let plain_entry = entries
            .iter()
            .find(|e| e.get("id").and_then(|i| i.as_str()) == Some("b"))
            .unwrap();
        assert!(plain_entry.get("group").is_none());
    }

    /// 改分组名：组里的主机一起改，其它组不动。
    #[test]
    fn rename_group_moves_all_members() {
        let store = HostsStore::new(&temp_dir());
        for (id, group) in [
            ("a", Some("生产环境")),
            ("b", Some("生产环境")),
            ("c", Some("测试环境")),
            ("d", None),
        ] {
            let mut h = sample(id);
            h.group = group.map(str::to_string);
            store.upsert(&h).unwrap();
        }

        let (hosts, groups) = store.rename_group("生产环境", "线上").unwrap();
        let group_of = |id: &str| {
            hosts
                .iter()
                .find(|h| h.id == id)
                .and_then(|h| h.group.clone())
                .unwrap_or_default()
        };
        assert_eq!(group_of("a"), "线上");
        assert_eq!(group_of("b"), "线上");
        assert_eq!(group_of("c"), "测试环境");
        assert_eq!(group_of("d"), "");

        // 清单里的分组也换名了
        assert_eq!(groups, vec!["线上".to_string(), "测试环境".to_string()]);

        // 落盘的也是新名字
        let hosts = store.load().unwrap();
        assert_eq!(
            hosts
                .iter()
                .find(|h| h.id == "a")
                .and_then(|h| h.group.clone()),
            Some("线上".to_string())
        );
    }

    /// 删除分组：主机回到未分组，主机本身不删。
    #[test]
    fn remove_group_keeps_hosts() {
        let store = HostsStore::new(&temp_dir());
        for (id, group) in [("a", Some("生产环境")), ("b", Some("生产环境"))] {
            let mut h = sample(id);
            h.group = group.map(str::to_string);
            store.upsert(&h).unwrap();
        }

        let (hosts, groups) = store.remove_group("生产环境").unwrap();
        assert_eq!(hosts.len(), 2, "删除分组不能删掉主机");
        assert!(hosts.iter().all(|h| h.group.is_none()));
        assert!(groups.is_empty(), "分组要从清单里消失");

        let hosts = store.load().unwrap();
        assert_eq!(hosts.len(), 2);
        assert!(hosts.iter().all(|h| h.group.is_none()));
    }

    /// 新建分组：可以是空分组 —— 用户先把「生产 / 测试」建好再往里放机器。
    #[test]
    fn create_group_allows_empty_group() {
        let store = HostsStore::new(&temp_dir());
        let (_, groups) = store.create_group("生产环境").unwrap();
        assert_eq!(groups, vec!["生产环境".to_string()]);

        // 再建一次同名的不重复
        let (_, groups) = store.create_group("生产环境").unwrap();
        assert_eq!(groups.len(), 1);

        // 存在磁盘上
        assert_eq!(store.groups().unwrap(), vec!["生产环境".to_string()]);
    }

    /// 主机上的分组名会自动进清单，顺序稳定。
    #[test]
    fn upsert_registers_group() {
        let store = HostsStore::new(&temp_dir());
        let mut h = sample("a");
        h.group = Some("测试环境".into());
        store.upsert(&h).unwrap();

        let mut h2 = sample("b");
        h2.group = Some("生产环境".into());
        store.upsert(&h2).unwrap();

        assert_eq!(
            store.groups().unwrap(),
            vec!["测试环境".to_string(), "生产环境".to_string()]
        );

        // 再加一台同组的机器，分组顺序不动
        let mut h3 = sample("c");
        h3.group = Some("测试环境".into());
        store.upsert(&h3).unwrap();
        assert_eq!(
            store.groups().unwrap(),
            vec!["测试环境".to_string(), "生产环境".to_string()]
        );
    }

    /// 旧版 hosts.json 没有 groups 字段，也要读得出来。
    #[test]
    fn legacy_file_without_groups_still_loads() {
        let dir = temp_dir();
        std::fs::write(
            dir.join("hosts.json"),
            r#"{"version":1,"hosts":[{"id":"a","label":"旧机器","host":"10.0.0.1","port":22,"username":"root","authType":"password","hasPassword":true,"hasPassphrase":false}]}"#,
        )
        .unwrap();
        let store = HostsStore::new(&dir);
        let hosts = store.load().unwrap();
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].label, "旧机器");
        assert!(store.groups().unwrap().is_empty());
    }

    /// 拖动排序：分组顺序按新的来，没提到的接在后面。
    #[test]
    fn reorder_groups_persists_new_order() {
        let store = HostsStore::new(&temp_dir());
        for name in ["甲", "乙", "丙"] {
            store.create_group(name).unwrap();
        }

        let (_, groups) = store
            .reorder_groups(&["丙".to_string(), "甲".to_string()])
            .unwrap();
        assert_eq!(
            groups,
            vec!["丙".to_string(), "甲".to_string(), "乙".to_string()]
        );

        // 落盘的还是新顺序
        assert_eq!(
            store.groups().unwrap(),
            vec!["丙".to_string(), "甲".to_string(), "乙".to_string()]
        );
    }

    /// 主机拖到别的分组；拖到不存在的分组就把它建出来。
    #[test]
    fn set_group_moves_host_and_creates_group() {
        let store = HostsStore::new(&temp_dir());
        let mut h = sample("a");
        h.group = Some("甲".into());
        store.upsert(&h).unwrap();

        let (hosts, groups) = store.set_group("a", Some("乙")).unwrap();
        assert_eq!(hosts[0].group.as_deref(), Some("乙"));
        assert!(groups.iter().any(|g| g == "乙"), "目标分组要进清单");

        // 拖回未分组
        let (hosts, groups) = store.set_group("a", None).unwrap();
        assert_eq!(hosts[0].group, None);
        assert!(groups.iter().any(|g| g == "乙"), "分组本身还在，只是空了");
    }

    /// 拖到不存在的机器上要报错，别默默把分组改了。
    #[test]
    fn set_group_rejects_unknown_host() {
        let store = HostsStore::new(&temp_dir());
        store.upsert(&sample("a")).unwrap();
        assert!(store.set_group("nope", Some("甲")).is_err());
        assert_eq!(store.load().unwrap()[0].group, None);
    }

    /// 缺陷 #1 的回归测试：落盘内容里不能出现任何明文秘密。
    #[test]
    fn persisted_file_contains_no_secrets() {
        let dir = temp_dir();
        let store = HostsStore::new(&dir);
        let mut host = sample("a");
        host.private_key_path = Some("/home/u/.ssh/id_rsa".into());
        store.upsert(&host).unwrap();

        let raw = std::fs::read_to_string(dir.join("hosts.json")).unwrap();

        // 结构检查：不存在 password / passphrase 这类字段。
        let json: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let entries = json.get("hosts").and_then(|h| h.as_array()).unwrap();
        for entry in entries {
            let obj = entry.as_object().unwrap();
            for forbidden in ["password", "passphrase", "secret", "token"] {
                assert!(
                    !obj.contains_key(forbidden),
                    "hosts.json 不应包含 {forbidden} 字段，实际键: {:?}",
                    obj.keys().collect::<Vec<_>>()
                );
            }
        }

        // 内容检查：秘密值本身不得出现在文件里。
        assert!(!raw.contains("hunter2"), "hosts.json 不得包含明文密码");
    }

    #[test]
    fn corrupt_file_is_backed_up_and_reset() {
        let dir = temp_dir();
        std::fs::write(dir.join("hosts.json"), "{ not json").unwrap();
        let store = HostsStore::new(&dir);
        assert!(store.load().unwrap().is_empty());
        assert!(dir.join("hosts.json.corrupt").exists());
    }
}
