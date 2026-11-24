//! 应用状态。由 Tauri 的 `manage` 注入，所有命令通过 `State<AppState>` 访问。

pub mod credentials;
pub mod host;
pub mod session;
pub mod sftp;

use tauri::AppHandle;

use crate::state::sessions::SessionRegistry;
use crate::storage::hosts::HostsStore;
use crate::storage::secrets::SecretStore;

/// 全局应用状态。
pub struct AppState {
    /// Tauri 应用句柄，用于事件广播。
    pub app: AppHandle,
    /// 活跃会话注册表。
    pub sessions: SessionRegistry,
    /// 已保存主机的存储。
    pub hosts: HostsStore,
    /// 凭据存储（系统钥匙串 / 本地加密文件）。
    pub secrets: SecretStore,
}

impl AppState {
    pub fn new(app: AppHandle, config_dir: std::path::PathBuf) -> Self {
        Self {
            app,
            sessions: SessionRegistry::new(),
            hosts: HostsStore::new(&config_dir),
            secrets: SecretStore::new(&config_dir),
        }
    }
}
