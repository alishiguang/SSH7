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
use russh::{ChannelMsg, ChannelReadHalf, ChannelWriteHalf, Pty};
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
    /// 这个会话属于**哪一扇窗**。
    ///
    /// 前端的状态是每扇窗一份（每扇 WebviewWindow 有独立的 JS 上下文），
    /// 所以「这个会话归谁」只能记在 Rust 侧 —— 它是所有窗口共享的唯一真相。
    /// 终端数据也只发给这扇窗，广播的话每扇窗都会收到别人的输出（缺陷 #31）。
    pub window_label: String,
    /// shell channel 的写端：用户输入与窗口尺寸调整都走这里。
    writer: Arc<ChannelWriteHalf<Msg>>,
    /// SSH 连接句柄。
    handle: Arc<Mutex<Handle<ClientHandler>>>,
    /// SFTP 客户端，全会话复用同一个 channel（缺陷 #4 / #5）。
    sftp: Arc<Mutex<Option<Arc<SftpClient>>>>,
    /// 前端是否已挂载终端。
    attached: AtomicBool,
    /// 连接是否已经关闭。
    ///
    /// 远端断开时连接会被释放，但会话仍留在注册表里（前端那边还显示着
    /// 一个「已断开」的标签）。这个标记用来区分「还活着」和「已经断了」——
    /// 否则「双击找已有窗口」会定位到一扇摆着死连接的窗。
    closed: AtomicBool,
    /// 挂载前缓存的终端数据。
    pending: Mutex<Vec<u8>>,
    /// 本次连接使用的凭据。
    ///
    /// 保留它是为了支持「克隆为新会话」：**凭据完全在 Rust 侧复用**，
    /// 不需要把密码传回前端再传回来。
    credential: Arc<crate::ssh::auth::Credential>,
}

/// 会话的「连接配置」标识。
///
/// 有主机 id 就按 id 认人 —— 两条配置指向同一台机器时，它们仍然是两台
/// 「机器」，各自独立开窗。没有 id（临时连的）就退回地址三元组。
pub fn target_key(host_id: Option<&str>, host: &str, port: u16, username: &str) -> String {
    match host_id {
        Some(id) if !id.trim().is_empty() => format!("id:{}", id.trim()),
        _ => format!("addr:{host}:{port}:{username}"),
    }
}

/// 把事件发给**拥有这个会话的那扇窗**。
///
/// 不能用广播：多窗口下广播会让每扇窗都收到别的会话的终端输出与传输进度，
/// 界面上会串台（缺陷 #31）。
fn emit_to_owner(
    app: &tauri::AppHandle,
    window_label: &str,
    event: &str,
    payload: impl serde::Serialize + Clone,
) {
    let _ = app.emit_to(
        tauri::EventTarget::Window {
            label: window_label.to_string(),
        },
        event,
        payload,
    );
}

impl Session {
    pub fn id(&self) -> &str {
        &self.id
    }

    /// 这个会话对应的是哪一条「连接配置」。
    pub fn target_key(&self) -> String {
        target_key(
            self.info.host_id.as_deref(),
            &self.info.host,
            self.info.port,
            &self.info.username,
        )
    }

    /// 连接是否已经断了。
    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Relaxed)
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
            emit_to_owner(app, &self.window_label, event::DATA, payload);
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
        emit_to_owner(app, &self.window_label, event::DATA, payload);
    }

    /// 断开连接并释放所有 channel。
    pub async fn shutdown(&self) {
        // 先标记为已关闭：找「已经开着的会话」时就不该再算上它。
        self.closed.store(true, Ordering::SeqCst);
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

/// 建立一次连接所需的全部参数。
///
/// 参数一多就容易传错位置 —— 打包成结构体，以后加字段也不会静默错位。
pub struct ConnectParams<'a> {
    pub host: &'a str,
    pub port: u16,
    pub username: &'a str,
    pub credential: &'a crate::ssh::auth::Credential,
    pub label: String,
    pub auth_type: crate::types::AuthType,
    /// 这个会话归哪扇窗 —— 多窗口下终端数据只发给它。
    pub window_label: String,
    /// 来自哪台已保存主机（连接配置的身份）。
    pub host_id: Option<String>,
}

/// 建立 SSH 连接并打开 shell 与 SFTP channel。
pub async fn connect(app: &tauri::AppHandle, p: ConnectParams<'_>) -> AppResult<Arc<Session>> {
    let ConnectParams {
        host,
        port,
        username,
        credential,
        label,
        auth_type,
        window_label,
        host_id,
    } = p;
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
    // **让服务端把 PTY 建好时就关掉回显**（RFC 4254 的 pty-req 终端模式）。
    //
    // 注入工作目录钩子时，脚本是写进 PTY 的**输入流**的：
    // 内核会回显一遍，bash 的 readline 启动时还会把读到的那行重绘一遍，
    // 于是用户每次开 session 都能在终端里看到几行 shell 代码（缺陷 #21）。
    //
    // 从一开始就没有回显，注入就完全看不见；装完钩子再用 `stty echo`
    // 把回显还回去（见 `tracker::install_command`）。
    channel
        .request_pty(true, "xterm-256color", 80, 24, 0, 0, &[(Pty::ECHO, 0)])
        .await
        .map_err(|e| map_ssh_error(&e))?;
    channel
        .request_shell(true)
        .await
        .map_err(|e| map_ssh_error(&e))?;

    let (read_half, writer) = channel.split();
    let writer = Arc::new(writer);
    let handle = Arc::new(Mutex::new(handle));

    // 探测远端 shell 并静默安装工作目录上报钩子。
    //
    // 放在后台做（缺陷 #22）：探测要额外开一个 exec channel，实测约 30ms，
    // 不该让用户干等着「连接中」。用独立的 exec channel 探测，
    // 不会污染交互式终端的输出。
    {
        let detect_handle = handle.clone();
        let hook_writer = writer.clone();
        tokio::spawn(async move {
            // 探测整体兜一个超时：远端不配合时不能一直占着连接句柄。
            let kind = tokio::time::timeout(Duration::from_secs(5), async {
                let guard = detect_handle.lock().await;
                detect_shell(&guard).await
            })
            .await
            .unwrap_or(None);
            // 认不出来、探测超时 —— 都照样要把回显还回去：回显是我们关的。
            let command = crate::ssh::tracker::install_command(
                kind.unwrap_or(crate::ssh::tracker::ShellKind::Unknown),
            );
            let _ = hook_writer
                .data_bytes(bytes::Bytes::from(command.into_bytes()))
                .await;
        });
    }

    let session_id = uuid::Uuid::now_v7().to_string();
    let info = SessionInfo {
        session_id: session_id.clone(),
        label,
        host: host.to_string(),
        port,
        username: username.to_string(),
        auth_type,
        host_id,
    };

    let session = Arc::new(Session {
        id: session_id.clone(),
        info: info.clone(),
        window_label,
        writer,
        handle,
        sftp: Arc::new(Mutex::new(None)),
        attached: AtomicBool::new(false),
        closed: AtomicBool::new(false),
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
pub async fn clone_session(
    app: &tauri::AppHandle,
    source: &Session,
    base_label: Option<&str>,
) -> AppResult<Arc<Session>> {
    // 用户可以在标签栏里给会话改名，克隆时沿用那个名字。
    let base = base_label
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(source.info.label.as_str());
    let label = format!("{base} · 副本");
    connect(
        app,
        ConnectParams {
            host: &source.info.host,
            port: source.info.port,
            username: &source.info.username,
            credential: &source.credential,
            label,
            auth_type: source.info.auth_type,
            // 克隆出来的会话属于**当前这扇窗**，并沿用源会话的连接配置身份。
            window_label: source.window_label.clone(),
            host_id: source.info.host_id.clone(),
        },
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
                    emit_to_owner(&app, &session.window_label, event::CWD, payload);
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
    emit_to_owner(&app, &session.window_label, event::CLOSED, payload);
}

/// 发送传输进度事件。
///
/// 进度只发给**发起这次传输的那扇窗** —— 别的窗既没发起也不该看见。
pub fn emit_transfer_progress(
    app: &tauri::AppHandle,
    window_label: &str,
    progress: TransferProgress,
) {
    emit_to_owner(app, window_label, event::TRANSFER, progress);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 有主机 id 就按 id 认人 —— 同一台机器的两条配置仍然是两台「机器」。
    #[test]
    fn target_key_prefers_host_id() {
        assert_eq!(target_key(Some("h1"), "10.0.0.1", 22, "root"), "id:h1");
        // 同一个地址、不同的 id → 两台机器。
        assert_ne!(
            target_key(Some("h1"), "10.0.0.1", 22, "root"),
            target_key(Some("h2"), "10.0.0.1", 22, "root")
        );
    }

    /// 没有 id（临时连的）就退回地址三元组。
    #[test]
    fn target_key_falls_back_to_address() {
        assert_eq!(
            target_key(None, "10.0.0.1", 22, "root"),
            "addr:10.0.0.1:22:root"
        );
        // 空 id 等同于没有 id。
        assert_eq!(
            target_key(Some(""), "10.0.0.1", 22, "root"),
            "addr:10.0.0.1:22:root"
        );
        assert_eq!(
            target_key(Some("   "), "10.0.0.1", 22, "root"),
            "addr:10.0.0.1:22:root"
        );
    }

    /// 地址三元组不同的，就是不同的目标。
    #[test]
    fn target_key_distinguishes_by_port_and_user() {
        assert_ne!(
            target_key(None, "10.0.0.1", 22, "root"),
            target_key(None, "10.0.0.1", 2222, "root")
        );
        assert_ne!(
            target_key(None, "10.0.0.1", 22, "root"),
            target_key(None, "10.0.0.1", 22, "ops")
        );
    }
}
