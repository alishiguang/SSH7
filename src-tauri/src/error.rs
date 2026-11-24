//! 统一错误模型。
//!
//! 旧版 Electron 实现把 `anyhow` 风格的裸字符串直接抛给渲染进程，
//! 前端只能显示「连接失败」而拿不到任何可操作信息（缺陷 #11）。
//! 这里用结构化的 `{ code, message, hint }` 取代裸字符串：前端据此
//! 渲染不同语气的提示，并给出修复建议。

use serde::{Serialize, Serializer};

/// 应用错误码。稳定契约，前端可按码分支处理。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    Timeout,
    AuthFailed,
    KeyInvalid,
    HostKeyRejected,
    ConnectionRefused,
    NetworkUnreachable,
    DnsFailed,
    NoSuchPath,
    PermissionDenied,
    NoSuchHost,
    SessionGone,
    KeyringUnavailable,
    Io,
    InvalidInput,
    Internal,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::Timeout => "TIMEOUT",
            ErrorCode::AuthFailed => "AUTH_FAILED",
            ErrorCode::KeyInvalid => "KEY_INVALID",
            ErrorCode::HostKeyRejected => "HOST_KEY_REJECTED",
            ErrorCode::ConnectionRefused => "CONNECTION_REFUSED",
            ErrorCode::NetworkUnreachable => "NETWORK_UNREACHABLE",
            ErrorCode::DnsFailed => "DNS_FAILED",
            ErrorCode::NoSuchPath => "NO_SUCH_PATH",
            ErrorCode::PermissionDenied => "PERMISSION_DENIED",
            ErrorCode::NoSuchHost => "NO_SUCH_HOST",
            ErrorCode::SessionGone => "SESSION_GONE",
            ErrorCode::KeyringUnavailable => "KEYRING_UNAVAILABLE",
            ErrorCode::Io => "IO",
            ErrorCode::InvalidInput => "INVALID_INPUT",
            ErrorCode::Internal => "INTERNAL",
        }
    }
}

/// 面向前端的错误载荷。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppErrorPayload {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

/// 应用内流转的错误类型。
#[derive(Debug, Clone, thiserror::Error)]
pub enum AppError {
    #[error("连接超时")]
    Timeout,

    #[error("认证失败")]
    AuthFailed(String),

    #[error("私钥无法解析")]
    KeyInvalid(String),

    #[error("主机密钥校验失败")]
    HostKeyRejected(String),

    #[error("连接被拒绝")]
    ConnectionRefused(String),

    #[error("网络不可达")]
    NetworkUnreachable(String),

    #[error("域名解析失败")]
    DnsFailed(String),

    #[error("远端路径不存在")]
    NoSuchPath(String),

    #[error("权限不足")]
    PermissionDenied(String),

    #[error("主机不存在")]
    NoSuchHost(String),

    #[error("会话已关闭")]
    SessionGone(String),

    #[error("系统钥匙串不可用")]
    KeyringUnavailable(String),

    #[error("本地文件操作失败")]
    Io(String),

    #[error("参数不合法")]
    InvalidInput(String),

    #[error("内部错误")]
    Internal(String),
}

impl AppError {
    pub fn code(&self) -> ErrorCode {
        match self {
            AppError::Timeout => ErrorCode::Timeout,
            AppError::AuthFailed(_) => ErrorCode::AuthFailed,
            AppError::KeyInvalid(_) => ErrorCode::KeyInvalid,
            AppError::HostKeyRejected(_) => ErrorCode::HostKeyRejected,
            AppError::ConnectionRefused(_) => ErrorCode::ConnectionRefused,
            AppError::NetworkUnreachable(_) => ErrorCode::NetworkUnreachable,
            AppError::DnsFailed(_) => ErrorCode::DnsFailed,
            AppError::NoSuchPath(_) => ErrorCode::NoSuchPath,
            AppError::PermissionDenied(_) => ErrorCode::PermissionDenied,
            AppError::NoSuchHost(_) => ErrorCode::NoSuchHost,
            AppError::SessionGone(_) => ErrorCode::SessionGone,
            AppError::KeyringUnavailable(_) => ErrorCode::KeyringUnavailable,
            AppError::Io(_) => ErrorCode::Io,
            AppError::InvalidInput(_) => ErrorCode::InvalidInput,
            AppError::Internal(_) => ErrorCode::Internal,
        }
    }

    /// 面向用户的操作建议。
    pub fn hint(&self) -> Option<String> {
        Some(match self {
            AppError::Timeout => "检查主机地址、端口与网络连通性，或在防火墙中放行该端口".into(),
            AppError::AuthFailed(_) => "核对用户名与密码；私钥认证时确认口令正确".into(),
            AppError::KeyInvalid(_) => "确认私钥格式为 OpenSSH / PEM，且未被服务端拒绝".into(),
            AppError::HostKeyRejected(_) => {
                "服务器主机密钥与记录不符，请确认是否更换过服务器".into()
            }
            AppError::ConnectionRefused(_) => {
                "目标端口没有服务在监听，核对 SSH 服务是否启动".into()
            }
            AppError::NetworkUnreachable(_) => "检查本机网络与路由是否可达该主机".into(),
            AppError::DnsFailed(_) => "检查域名拼写，或改用 IP 地址连接".into(),
            AppError::NoSuchPath(_) => "路径可能已被删除或改名，可返回上级目录重新浏览".into(),
            AppError::PermissionDenied(_) => "远端账号对该路径没有读写权限，可换一个目录".into(),
            AppError::NoSuchHost(_) => "该主机已从列表中移除".into(),
            AppError::SessionGone(_) => "连接已断开，重新建立会话".into(),
            AppError::KeyringUnavailable(_) => {
                "当前系统没有可用的密钥库，本次连接仍然有效，但密码不会被保存".into()
            }
            AppError::Io(_) => "检查本地路径是否存在以及读写权限".into(),
            AppError::InvalidInput(_) => "修正表单中的字段后再试".into(),
            AppError::Internal(_) => return None,
        })
    }

    pub fn payload(&self) -> AppErrorPayload {
        AppErrorPayload {
            code: self.code().as_str().to_string(),
            message: self.to_string(),
            hint: self.hint(),
        }
    }
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.payload().serialize(serializer)
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::Io(e.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;

/// 把 `russh` 的错误归类到结构化错误码。
pub fn map_ssh_error(err: &russh::Error) -> AppError {
    use russh::Error as E;
    match err {
        E::IO(e) => match e.kind() {
            std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => AppError::Timeout,
            std::io::ErrorKind::ConnectionRefused => AppError::ConnectionRefused(e.to_string()),
            std::io::ErrorKind::NetworkUnreachable => AppError::NetworkUnreachable(e.to_string()),
            std::io::ErrorKind::NotFound => AppError::DnsFailed(e.to_string()),
            std::io::ErrorKind::PermissionDenied => AppError::PermissionDenied(e.to_string()),
            _ => AppError::Internal(e.to_string()),
        },
        E::ConnectionTimeout => AppError::Timeout,
        E::NoAuthMethod => AppError::AuthFailed("服务器不接受任何可用的认证方式".into()),
        E::RequestDenied => AppError::AuthFailed("服务器拒绝了认证请求".into()),
        E::KeyChanged { line } => AppError::HostKeyRejected(format!(
            "服务器主机密钥与第 {line} 行的记录不一致，可能遭遇中间人攻击"
        )),
        E::Keys(k) => AppError::KeyInvalid(k.to_string()),
        E::HUP => AppError::Internal("连接被对端关闭".into()),
        E::Disconnect => AppError::Internal("连接被对端断开".into()),
        other => AppError::Internal(format!("{other:?}")),
    }
}
