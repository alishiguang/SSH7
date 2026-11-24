//! 前后端共享的数据类型。
//!
//! 所有结构体都使用 `camelCase` 序列化，与 `src/api/types.ts` 一一对应。

use serde::{Deserialize, Serialize};

use crate::error::AppErrorPayload;

/// 认证方式。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AuthType {
    Password,
    PrivateKey,
}

/// 建立连接的请求。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectRequest {
    #[serde(default)]
    pub label: Option<String>,
    /// 保存主机时使用的分组名，留空表示不分组。
    #[serde(default)]
    pub group: Option<String>,
    pub host: String,
    #[serde(default)]
    pub port: Option<u16>,
    pub username: String,
    pub auth_type: AuthType,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub private_key_path: Option<String>,
    #[serde(default)]
    pub passphrase: Option<String>,
    /// 是否把主机保存到列表。
    #[serde(default)]
    pub save: Option<bool>,
    /// 保存时用于覆盖已有主机。
    #[serde(default)]
    pub host_id: Option<String>,
}

/// 一次成功建立的会话。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub session_id: String,
    pub label: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    /// 本次使用的认证方式，前端据此实现「重试」。
    pub auth_type: AuthType,
}

/// 连通性测试结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestResult {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<AppErrorPayload>,
}

/// 远端文件条目。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteFile {
    pub name: String,
    /// 完整远端路径。
    pub path: String,
    pub is_directory: bool,
    pub is_symlink: bool,
    pub size: u64,
    /// Unix 秒级时间戳。
    pub modified_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permissions: Option<u32>,
}

/// 传输方向。
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TransferDirection {
    Upload,
    Download,
}

/// 传输进度事件。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferProgress {
    pub transfer_id: String,
    pub session_id: String,
    pub file_name: String,
    pub transferred: u64,
    pub total: u64,
    /// 0..=100，已夹取，不会出现 NaN（缺陷 #13）。
    pub percent: u8,
    pub direction: TransferDirection,
}

/// 终端数据事件。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDataEvent {
    pub session_id: String,
    pub data: String,
}

/// 当前目录变化事件。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionCwdEvent {
    pub session_id: String,
    pub cwd: String,
}

/// 会话关闭事件。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionClosedEvent {
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// 会话错误事件。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionErrorEvent {
    pub session_id: String,
    pub error: AppErrorPayload,
}

/// 前端保存主机时提交的内容。**不含**任何明文秘密字段之外的存储逻辑，
/// 秘密单独走钥匙串。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostSaveRequest {
    pub host: SavedHostDto,
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub passphrase: Option<String>,
}

/// 已保存主机的传输对象。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedHostDto {
    pub id: String,
    pub label: String,
    /// 所属分组，留空表示不分组。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_type: AuthType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private_key_path: Option<String>,
    #[serde(default)]
    pub has_password: bool,
    #[serde(default)]
    pub has_passphrase: bool,
}

/// 主机列表 + 分组清单。
///
/// 分组清单里可以有**空分组**（还没放任何主机），
/// 这样用户可以先把「生产 / 测试 / 本地」建好，往里放机器时直接选。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostListDto {
    pub hosts: Vec<SavedHostDto>,
    /// 分组名与显示顺序。主机上的 `group` 字段指向这里的名字。
    #[serde(default)]
    pub groups: Vec<String>,
}

/// 事件名常量。前端 `src/api/bridge.ts` 中的 `EVENT` 必须与之对应。
pub mod event {
    pub const DATA: &str = "session://data";
    pub const CLOSED: &str = "session://closed";
    pub const CWD: &str = "session://cwd";
    pub const ERROR: &str = "session://error";
    pub const TRANSFER: &str = "transfer://progress";
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 前端 `HostList` 与 Rust 侧 `HostListDto` 的字段名必须对得上。
    /// 改了一边忘了另一边，界面就会静悄悄地少显示东西。
    #[test]
    fn host_list_dto_uses_camel_case() {
        let dto = HostListDto {
            hosts: vec![SavedHostDto {
                id: "a".into(),
                label: "机器".into(),
                group: Some("生产环境".into()),
                host: "10.0.0.1".into(),
                port: 22,
                username: "root".into(),
                auth_type: AuthType::Password,
                private_key_path: None,
                has_password: true,
                has_passphrase: false,
            }],
            groups: vec!["生产环境".into()],
        };

        let json: serde_json::Value = serde_json::to_value(&dto).unwrap();
        assert!(json.get("hosts").is_some(), "缺少 hosts 字段");
        assert!(json.get("groups").is_some(), "缺少 groups 字段");

        let host = &json["hosts"][0];
        assert_eq!(host["authType"], "password");
        assert_eq!(host["hasPassword"], true);
        assert_eq!(host["group"], "生产环境");
        // 没有的字段不要出现，免得前端拿到一堆 null。
        assert!(host.get("privateKeyPath").is_none());
    }
}
