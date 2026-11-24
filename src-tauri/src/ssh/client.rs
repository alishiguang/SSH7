//! SSH 连接与会话生命周期。
//!
//! 相比旧版实现的关键改动：
//!
//! - 会话由 [`Session`] 统一持有 shell channel、SFTP channel 与缓冲，
//!   关闭标签页时整体释放（缺陷 #2 / #4 / #5 / #9）。
//! - 终端数据在「前端尚未挂载」时先缓存，前端 attach 后再补发，
//!   避免首批输出丢失。
//! - 主机密钥校验：首次连接记录到 `~/.ssh/known_hosts`，之后变更即拒绝。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use russh::client::{Config, Handle, Msg};
use russh::keys::{PrivateKeyWithHashAlg, PublicKey};
use russh::{ChannelMsg, ChannelReadHalf, ChannelWriteHalf};
use tauri::Emitter;
use tokio::sync::Mutex;

use crate::error::{map_ssh_error, AppError, AppResult};
use crate::ssh::osc7::Osc7Parser;
use crate::ssh::sftp::SftpClient;
use crate::types::{
    event, SessionClosedEvent, SessionCwdEvent, SessionDataEvent, SessionInfo, TransferProgress,
};

/// 连接超时。旧版是硬编码 10s 且在私钥读取失败后不清理定时器（缺陷 #18）。
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// 未 attach 前缓存的终端数据上限。
const MAX_PENDING_BYTES: usize = 256 * 1024;

/// 客户端处理器：负责主机密钥校验。
pub struct ClientHandler {
    pub host: String,
    pub port: u16,
}

impl russh::client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &russh::keys::PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let key: PublicKey = server_public_key.public_key();

        match russh::keys::check_known_hosts(&self.host, self.port, &key) {
            Ok(true) => Ok(true),
            Ok(false) => {
                // 首次连接：记录到 known_hosts（TOFU）。
                if let Err(e) =
                    russh::keys::known_hosts::learn_known_hosts(&self.host, self.port, &key)
                {
                    tracing::warn!(error = %e, "无法写入 known_hosts");
                }
                Ok(true)
            }
            Err(russh::keys::Error::KeyChanged { line }) => {
                tracing::error!(line, "服务器主机密钥已变更，拒绝连接");
                Err(russh::Error::KeyChanged { line })
            }
            Err(e) => {
                // known_hosts 读取失败不应阻断连接，退回到 TOFU。
                tracing::warn!(error = %e, "读取 known_hosts 失败");
                if let Err(e) =
                    russh::keys::known_hosts::learn_known_hosts(&self.host, self.port, &key)
                {
                    tracing::warn!(error = %e, "无法写入 known_hosts");
                }
                Ok(true)
            }
        }
    }
}

/// 一个活跃的 SSH 会话。
pub struct Session {
    pub id: String,
    pub info: SessionInfo,
    /// shell channel 的写端：用户输入与窗口尺寸调整都走这里。
    writer: Arc<ChannelWriteHalf<Msg>>,
    /// SSH 连接句柄。
    handle: Arc<Mutex<Handle<ClientHandler>>>,
    /// SFTP 客户端，全会话复用同一个 channel（缺陷 #4 / #5）。
    sftp: Arc<Mutex<Option<Arc<SftpClient>>>>,
    /// 前端是否已挂载终端。
    attached: AtomicBool,
    /// 挂载前缓存的终端数据。
    pending: Mutex<Vec<u8>>,
    /// 本次连接使用的凭据。
    ///
    /// 保留它是为了支持「克隆为新会话」：**凭据完全在 Rust 侧复用**，
    /// 不需要把密码传回前端再传回来。
    credential: Arc<crate::ssh::auth::Credential>,
}

impl Session {
    pub fn id(&self) -> &str {
        &self.id
    }

    /// 写入终端输入。
    pub async fn write(&self, data: &[u8]) -> AppResult<()> {
        self.writer
            .data_bytes(bytes::Bytes::copy_from_slice(data))
            .await
            .map_err(|e| AppError::Internal(format!("写入终端失败: {e}")))
    }

    /// 调整终端尺寸。
    pub async fn resize(&self, cols: u32, rows: u32) -> AppResult<()> {
        self.writer
            .window_change(cols, rows, 0, 0)
            .await
            .map_err(|e| AppError::Internal(format!("调整终端尺寸失败: {e}")))
    }

    /// 取得（必要时创建）SFTP 客户端。
    pub async fn sftp(&self) -> AppResult<Arc<SftpClient>> {
        let mut guard = self.sftp.lock().await;
        if let Some(s) = guard.as_ref() {
            return Ok(s.clone());
        }
        let client = SftpClient::open(self.handle.clone()).await?;
        *guard = Some(client.clone());
        Ok(client)
    }
    /// 缓存或直接投递终端数据。
    pub async fn push_data(&self, app: &tauri::AppHandle, data: &[u8]) {
        if self.attached.load(Ordering::Relaxed) {
            let payload = SessionDataEvent {
                session_id: self.id.clone(),
                data: String::from_utf8_lossy(data).into_owned(),
            };
            let _ = app.emit(event::DATA, payload);
        } else {
            let mut buf = self.pending.lock().await;
            if buf.len() < MAX_PENDING_BYTES {
                buf.extend_from_slice(data);
            }
        }
    }

    /// 前端挂载终端后调用：补发缓存数据。
    pub async fn attach(&self, app: &tauri::AppHandle) {
        if self.attached.swap(true, Ordering::SeqCst) {
            return;
        }
        let buffered = {
            let mut buf = self.pending.lock().await;
            std::mem::take(&mut *buf)
        };
        if buffered.is_empty() {
            return;
        }
        let payload = SessionDataEvent {
            session_id: self.id.clone(),
            data: String::from_utf8_lossy(&buffered).into_owned(),
        };
        let _ = app.emit(event::DATA, payload);
    }

    /// 断开连接并释放所有 channel。
    pub async fn shutdown(&self) {
        let _ = self.writer.close().await;
        if let Some(sftp) = self.sftp.lock().await.as_ref() {
            sftp.close().await;
        }
        let handle = self.handle.lock().await;
        let _ = handle
            .disconnect(russh::Disconnect::ByApplication, "", "en")
            .await;
    }
}

/// 建立 SSH 连接并打开 shell 与 SFTP channel。
pub async fn connect(
    app: &tauri::AppHandle,
    host: &str,
    port: u16,
    username: &str,
    credential: &crate::ssh::auth::Credential,
    label: String,
    auth_type: crate::types::AuthType,
) -> AppResult<Arc<Session>> {
    let handler = ClientHandler {
        host: host.to_string(),
        port,
    };
    let config = Arc::new(Config {
        inactivity_timeout: Some(Duration::from_secs(3600)),
        keepalive_interval: Some(Duration::from_secs(30)),
        keepalive_max: 3,
        nodelay: true,
        ..Default::default()
    });

    let (host_str, port) = (host.to_string(), port);
    let connect_fut = russh::client::connect(config, (host_str.as_str(), port), handler);

    let mut handle = tokio::time::timeout(CONNECT_TIMEOUT, connect_fut)
        .await
        .map_err(|_| AppError::Timeout)?
        .map_err(|e| map_ssh_error(&e))?;

    // 认证
    let auth_result = match credential {
        crate::ssh::auth::Credential::Password(pw) => handle
            .authenticate_password(username, pw.as_str())
            .await
            .map_err(|e| map_ssh_error(&e))?,
        crate::ssh::auth::Credential::PrivateKey { key } => {
            let with_alg = PrivateKeyWithHashAlg::new(key.clone(), None);
            handle
                .authenticate_publickey(username, with_alg)
                .await
                .map_err(|e| map_ssh_error(&e))?
        }
    };

    if !auth_result.success() {
        return Err(AppError::AuthFailed(format!(
            "服务器拒绝了 {username}@{host}:{port} 的认证"
        )));
    }

    // shell channel
    let channel = handle
        .channel_open_session()
        .await
        .map_err(|e| map_ssh_error(&e))?;
    channel
        .request_pty(true, "xterm-256color", 80, 24, 0, 0, &[])
        .await
        .map_err(|e| map_ssh_error(&e))?;
    channel
        .request_shell(true)
        .await
        .map_err(|e| map_ssh_error(&e))?;

    let (read_half, writer) = channel.split();

    // 探测远端 shell 并注入工作目录上报钩子。
    // 用独立的 exec channel 探测，不会污染交互式终端的输出。
    let shell = detect_shell(&handle).await;
    if let Some(kind) = shell {
        if let Some(script) = crate::ssh::tracker::hook_script(kind) {
            writer
                .data_bytes(bytes::Bytes::from(script.as_bytes()))
                .await
                .ok();
        }
    }

    let session_id = uuid::Uuid::now_v7().to_string();
    let info = SessionInfo {
        session_id: session_id.clone(),
        label,
        host: host.to_string(),
        port,
        username: username.to_string(),
        auth_type,
    };

    let session = Arc::new(Session {
        id: session_id.clone(),
        info: info.clone(),
        writer: Arc::new(writer),
        handle: Arc::new(Mutex::new(handle)),
        sftp: Arc::new(Mutex::new(None)),
        attached: AtomicBool::new(false),
        pending: Mutex::new(Vec::new()),
        credential: Arc::new(credential.clone()),
    });

    // 数据泵：读取 shell 输出，解析 OSC 7，投递到前端。
    let pump_session = session.clone();
    let pump_app = app.clone();
    tokio::spawn(async move {
        pump(pump_app, pump_session, read_half).await;
    });

    Ok(session)
}

/// 探测远端交互式 shell 的类型。
///
/// 通过一个独立的 exec channel 执行 `echo $0` 来判断 —— sshd 会用用户的
/// 登录 shell 承载 exec 请求，因此 `$0` 就是该 shell 的路径。
/// 这样做的好处是**不会在交互式终端里产生任何可见输出**。
async fn detect_shell(handle: &Handle<ClientHandler>) -> Option<crate::ssh::tracker::ShellKind> {
    use tokio::io::AsyncReadExt;

    let channel = handle.channel_open_session().await.ok()?;
    channel
        .exec(true, crate::ssh::tracker::DETECT_COMMAND)
        .await
        .ok()?;
    let mut stream = channel.into_stream();

    let mut buf = Vec::with_capacity(128);
    let read = tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut buf)).await;
    if read.is_err() {
        return None;
    }

    let raw = String::from_utf8_lossy(&buf);
    let line = raw.lines().next()?.trim();
    if line.is_empty() {
        return None;
    }
    Some(crate::ssh::tracker::parse_detect_output(line))
}

/// 以现有会话为模板建立一个新会话。
///
/// 凭据直接复用 Rust 侧持有的副本，**密码不会回到前端**，
/// 因此克隆操作不需要用户重新输入密码。
pub async fn clone_session(app: &tauri::AppHandle, source: &Session) -> AppResult<Arc<Session>> {
    let label = format!("{} · 副本", source.info.label);
    connect(
        app,
        &source.info.host,
        source.info.port,
        &source.info.username,
        &source.credential,
        label,
        source.info.auth_type,
    )
    .await
}

/// shell 输出泵。
async fn pump(app: tauri::AppHandle, session: Arc<Session>, mut read_half: ChannelReadHalf) {
    let mut parser = Osc7Parser::new();
    let mut reason: Option<String> = None;

    while let Some(msg) = read_half.wait().await {
        match msg {
            ChannelMsg::Data { data } => {
                if let Some(cwd) = parser.feed(&data) {
                    let payload = SessionCwdEvent {
                        session_id: session.id.clone(),
                        cwd,
                    };
                    let _ = app.emit(event::CWD, payload);
                }
                session.push_data(&app, &data).await;
            }
            ChannelMsg::ExtendedData { data, .. } => {
                session.push_data(&app, &data).await;
            }
            ChannelMsg::Eof => {
                reason = Some("远端关闭了连接".into());
                break;
            }
            ChannelMsg::Close => {
                reason = Some("连接已关闭".into());
                break;
            }
            _ => {}
        }
    }

    // 关闭会话，释放资源并通知前端。
    session.shutdown().await;
    let payload = SessionClosedEvent {
        session_id: session.id.clone(),
        reason,
    };
    let _ = app.emit(event::CLOSED, payload);
}

/// 发送传输进度事件。
pub fn emit_transfer_progress(app: &tauri::AppHandle, progress: TransferProgress) {
    let _ = app.emit(event::TRANSFER, progress);
}
