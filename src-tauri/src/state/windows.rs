//! 窗口登记表。
//!
//! 多窗口下前端的状态是**每扇窗一份** —— 每扇 WebviewWindow 都有独立的
//! JS 上下文，各自有独立的 zustand store。所以「哪台机器开在哪扇窗」、
//! 「这台机器是不是已经开过窗」这些事实，**只能记在 Rust 侧**：
//! 它是所有窗口共享的唯一真相。
//!
//! 登记两件事：
//!
//! - **指派**：这扇窗是专门为哪台机器开的（启动后自动连上它）；
//! - **反查**：这台机器已经开过哪几扇窗（双击时用来定位到已有的那扇）。
//!
//! 已经建立的会话各自属于哪扇窗，记在 [`crate::ssh::client::Session`]
//! 自己身上 —— 它跟着连接一起生灭。

use dashmap::DashMap;
use serde::Serialize;

use crate::ssh::client::target_key;

/// 一扇窗被指派去连哪台机器。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowAssignment {
    /// 已保存主机的 id。有它就能从主机列表里把连接参数捞回来。
    pub host_id: Option<String>,
    pub host: String,
    pub port: u16,
    pub username: String,
    /// 窗口标题上显示的名字。
    pub label: Option<String>,
}

impl WindowAssignment {
    /// 连接配置标识：优先按 id 认人。
    pub fn key(&self) -> String {
        target_key(
            self.host_id.as_deref(),
            &self.host,
            self.port,
            &self.username,
        )
    }
}

/// 窗口登记表。
#[derive(Default)]
pub struct WindowRegistry {
    /// 窗口标签 → 这扇窗的指派。
    assignments: DashMap<String, WindowAssignment>,
    /// 连接配置标识 → 已经为它开过的窗口标签。
    by_key: DashMap<String, Vec<String>>,
}

impl WindowRegistry {
    pub fn new() -> Self {
        Self {
            assignments: DashMap::new(),
            by_key: DashMap::new(),
        }
    }

    /// 记下这扇窗是专门为哪台机器开的。
    pub fn assign(&self, window_label: &str, assignment: WindowAssignment) {
        let key = assignment.key();
        self.assignments
            .insert(window_label.to_string(), assignment);
        self.by_key
            .entry(key)
            .or_default()
            .push(window_label.to_string());
    }

    /// 这扇窗的指派（没有就是普通窗口）。
    pub fn assignment(&self, window_label: &str) -> Option<WindowAssignment> {
        self.assignments.get(window_label).map(|v| v.clone())
    }

    /// 已经为这台机器开过哪几扇窗。
    pub fn windows_for(&self, key: &str) -> Vec<String> {
        self.by_key.get(key).map(|v| v.clone()).unwrap_or_default()
    }

    /// 窗口没了就把它记着的东西清掉，别留悬空的条目。
    pub fn forget(&self, window_label: &str) {
        if let Some((_, assignment)) = self.assignments.remove(window_label) {
            let key = assignment.key();
            if let Some(mut list) = self.by_key.get_mut(&key) {
                list.retain(|w| w != window_label);
                if list.is_empty() {
                    drop(list);
                    self.by_key.remove(&key);
                }
            }
        }
    }

    /// 当前登记着多少扇待连接的窗口（测试用）。
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.assignments.len()
    }

    /// 一扇窗都没登记（测试用）。
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.assignments.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assignment(host_id: &str) -> WindowAssignment {
        WindowAssignment {
            host_id: Some(host_id.to_string()),
            host: "10.0.0.1".into(),
            port: 22,
            username: "root".into(),
            label: Some("机器".into()),
        }
    }

    #[test]
    fn assignment_is_found_by_window_label() {
        let reg = WindowRegistry::new();
        reg.assign("w-1", assignment("host-a"));
        let got = reg.assignment("w-1").expect("应当能取到指派");
        assert_eq!(got.host_id.as_deref(), Some("host-a"));
        assert_eq!(reg.assignment("w-2"), None);
    }

    #[test]
    fn windows_for_finds_opened_windows() {
        let reg = WindowRegistry::new();
        reg.assign("w-1", assignment("host-a"));
        reg.assign("w-2", assignment("host-a"));
        reg.assign("w-3", assignment("host-b"));

        let mut for_a = reg.windows_for(&assignment("host-a").key());
        for_a.sort();
        assert_eq!(for_a, vec!["w-1".to_string(), "w-2".to_string()]);
        assert_eq!(reg.windows_for(&assignment("host-b").key()).len(), 1);
    }

    #[test]
    fn forget_removes_window_from_both_maps() {
        let reg = WindowRegistry::new();
        reg.assign("w-1", assignment("host-a"));
        reg.assign("w-2", assignment("host-a"));
        reg.forget("w-1");

        assert_eq!(reg.assignment("w-1"), None);
        assert_eq!(
            reg.windows_for(&assignment("host-a").key()),
            vec!["w-2".to_string()]
        );
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn key_prefers_host_id_over_address() {
        let mut a = assignment("host-a");
        a.host = "10.0.0.9".into();
        let mut b = assignment("host-b");
        b.host = "10.0.0.9".into();
        // 同一个地址、不同的主机 id —— 是两台「机器」。
        assert_ne!(a.key(), b.key());

        let c = WindowAssignment {
            host_id: None,
            host: "10.0.0.9".into(),
            port: 22,
            username: "root".into(),
            label: None,
        };
        // 没有 id 时退回地址三元组。
        assert_eq!(c.key(), "addr:10.0.0.9:22:root");
    }
}
