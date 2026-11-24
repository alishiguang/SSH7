//! 会话注册表。
//!
//! 缺陷 #2 / #9 的修复核心：会话的**创建与销毁**都在这里统一管理。
//! 前端关闭标签页时会调用 `session_disconnect`，后端从注册表移除会话并
//! 释放 shell channel、SFTP channel 与 TCP 连接。
//! 旧版只是把标签从 UI 上移除，SSH 连接一直挂在后台。

use std::sync::Arc;

use dashmap::DashMap;

use crate::ssh::client::Session;

/// 会话注册表。线程安全，支持并发访问。
#[derive(Default)]
pub struct SessionRegistry {
    sessions: DashMap<String, Arc<Session>>,
}

impl SessionRegistry {
    pub fn new() -> Self {
        Self {
            sessions: DashMap::new(),
        }
    }

    pub fn insert(&self, id: &str, session: Arc<Session>) {
        self.sessions.insert(id.to_string(), session);
    }

    pub fn get(&self, id: &str) -> Option<Arc<Session>> {
        self.sessions.get(id).map(|v| v.clone())
    }

    /// 移除并返回会话。调用方负责断开连接。
    pub fn remove(&self, id: &str) -> Option<Arc<Session>> {
        self.sessions.remove(id).map(|(_, v)| v)
    }

    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }

    /// 所有会话 id。
    pub fn ids(&self) -> Vec<String> {
        self.sessions.iter().map(|e| e.key().clone()).collect()
    }

    /// 断开并清空所有会话（窗口关闭时使用）。
    pub async fn shutdown_all(&self) {
        let sessions: Vec<Arc<Session>> = self.sessions.iter().map(|e| e.value().clone()).collect();
        self.sessions.clear();
        for s in sessions {
            s.shutdown().await;
        }
    }

    /// 断开属于**某一扇窗**的所有会话。
    ///
    /// 多窗口下这是必须的：关掉一扇窗，它自己的会话得跟着断，
    /// 不然连接会一直挂在后台（缺陷 #2 / #9 在多窗口下的延伸）。
    pub async fn shutdown_window(&self, window_label: &str) {
        let doomed: Vec<String> = self
            .sessions
            .iter()
            .filter(|e| e.value().window_label == window_label)
            .map(|e| e.key().clone())
            .collect();
        for id in doomed {
            if let Some(s) = self.remove(&id) {
                s.shutdown().await;
            }
        }
    }

    /// 属于某一扇窗的会话 id。
    pub fn ids_of_window(&self, window_label: &str) -> Vec<String> {
        self.sessions
            .iter()
            .filter(|e| e.value().window_label == window_label)
            .map(|e| e.key().clone())
            .collect()
    }
}
