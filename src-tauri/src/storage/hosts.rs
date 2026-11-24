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
    hosts: Vec<SavedHost>,
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
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let raw = std::fs::read_to_string(&self.path)?;
        if raw.trim().is_empty() {
            return Ok(Vec::new());
        }
        match serde_json::from_str::<HostsFile>(&raw) {
            Ok(f) => Ok(f.hosts),
            Err(e) => {
                // 文件损坏时不让整个应用起不来：备份后从空列表开始。
                let backup = self.path.with_extension("json.corrupt");
                let _ = std::fs::rename(&self.path, &backup);
                tracing::warn!(error = %e, "hosts.json 解析失败，已备份并重置");
                Ok(Vec::new())
            }
        }
    }

    /// 原子写入：先写临时文件再重命名，避免中途崩溃留下半截 JSON。
    pub fn save(&self, hosts: &[SavedHost]) -> AppResult<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = HostsFile {
            version: 1,
            hosts: hosts.to_vec(),
        };
        let body = serde_json::to_string_pretty(&file)
            .map_err(|e| AppError::Internal(format!("序列化主机列表失败: {e}")))?;

        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, body)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }

    /// 新增或更新（按 id 匹配）。
    pub fn upsert(&self, host: &SavedHost) -> AppResult<Vec<SavedHost>> {
        let mut hosts = self.load()?;
        match hosts.iter_mut().find(|h| h.id == host.id) {
            Some(existing) => *existing = host.clone(),
            None => hosts.push(host.clone()),
        }
        self.save(&hosts)?;
        Ok(hosts)
    }

    /// 按 id 删除。
    pub fn remove(&self, id: &str) -> AppResult<Vec<SavedHost>> {
        let mut hosts = self.load()?;
        hosts.retain(|h| h.id != id);
        self.save(&hosts)?;
        Ok(hosts)
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
