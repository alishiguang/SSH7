//! 会话命令：连接、测试、输入、尺寸、断开。

use std::time::Instant;

use tauri::State;

use crate::commands::host::{load_passphrase, load_password};
use crate::commands::AppState;
use crate::error::{AppError, AppResult};
use crate::ssh::auth::{load_private_key, Credential};
use crate::ssh::client;
use crate::storage::secrets::SecretStore;
use crate::types::{AuthType, ConnectRequest, SessionInfo, TestResult};

/// 解析认证凭据。
///
/// 密码与私钥口令的来源优先级：
/// 1. 本次请求里显式传入的值；
/// 2. 凭据库中为该 `host_id` 保存的值（复用已保存主机时走这条路径）。
///
/// 这样前端在任何情况下都拿不到明文秘密。
fn resolve_credential(request: &ConnectRequest, store: &SecretStore) -> AppResult<Credential> {
    match request.auth_type {
        AuthType::Password => {
            let password = match request.password.as_ref().filter(|p| !p.is_empty()) {
                Some(p) => Some(p.clone()),
                None => match request.host_id.as_ref() {
                    Some(id) => load_password(store, id)?,
                    None => None,
                },
            };
            match password {
                Some(p) => Ok(Credential::Password(p)),
                None => Err(AppError::AuthFailed(
                    "未提供密码，且本机没有保存该主机的密码".into(),
                )),
            }
        }
        AuthType::PrivateKey => {
            let path = request
                .private_key_path
                .as_ref()
                .filter(|p| !p.is_empty())
                .ok_or_else(|| AppError::InvalidInput("未提供私钥路径".into()))?;

            let passphrase = match request.passphrase.as_ref().filter(|p| !p.is_empty()) {
                Some(p) => Some(p.clone()),
                None => match request.host_id.as_ref() {
                    Some(id) => load_passphrase(store, id)?,
                    None => None,
                },
            };

            let key = load_private_key(path, passphrase.as_deref())?;
            Ok(Credential::PrivateKey { key })
        }
    }
}

/// 建立连接。
#[tauri::command]
pub async fn session_connect(
    state: State<'_, AppState>,
    window: tauri::Window,
    request: ConnectRequest,
) -> AppResult<SessionInfo> {
    let host = request.host.trim().to_string();
    if host.is_empty() {
        return Err(AppError::InvalidInput("主机地址不能为空".into()));
    }
    let username = request.username.trim().to_string();
    if username.is_empty() {
        return Err(AppError::InvalidInput("用户名不能为空".into()));
    }
    let port = request.port.unwrap_or(22);
    if port == 0 {
        return Err(AppError::InvalidInput("端口不能为 0".into()));
    }

    let credential = resolve_credential(&request, &state.secrets)?;
    let label = request
        .label
        .as_ref()
        .filter(|l| !l.trim().is_empty())
        .cloned()
        .unwrap_or_else(|| format!("{username}@{host}"));

    // 主机 id **提前**定下来：session 内部的「连接配置身份」和返回给前端的
    // 用的是同一个值。
    //
    // 前端要靠这个 id 从加密凭据库里取回密码 —— 少了它，断线之后就再也
    // 连不上了（缺陷 #43）。原来这个 id 是「保存主机时」才生成的，前端
    // 压根拿不到：手动新建的主机，密码进了凭据库，却没人知道它的 id。
    let host_id = resolve_host_id(&request);

    let session = client::connect(
        &state.app,
        client::ConnectParams {
            host: &host,
            port,
            username: &username,
            credential: &credential,
            label: label.clone(),
            auth_type: request.auth_type,
            // 这个会话归**发起连接的这扇窗**所有 —— 终端数据只发给它。
            window_label: window.label().to_string(),
            host_id: host_id.clone(),
        },
    )
    .await?;

    let info = session.info.clone();
    state.sessions.insert(&info.session_id, session);

    // 保存主机与秘密（秘密进凭据库，hosts.json 不含明文）。
    if request.save.unwrap_or(false) {
        let host_id = host_id
            .clone()
            .unwrap_or_else(|| uuid::Uuid::now_v7().to_string());

        let saved = crate::storage::hosts::SavedHost {
            id: host_id.clone(),
            label: label.clone(),
            group: request.group.clone().filter(|g| !g.trim().is_empty()),
            host: host.clone(),
            port,
            username: username.clone(),
            auth_type: request.auth_type,
            private_key_path: request.private_key_path.clone().filter(|p| !p.is_empty()),
            has_password: matches!(&credential, Credential::Password(_)),
            has_passphrase: request.passphrase.is_some(),
        };
        state.hosts.upsert(&saved)?;

        // 把本次使用的密码写进凭据库，供下次复用。
        if let Credential::Password(pw) = &credential {
            let _ = state
                .secrets
                .set(&crate::storage::secrets::password_account(&host_id), pw);
        }
        if let Some(pp) = request.passphrase.as_ref().filter(|p| !p.is_empty()) {
            let _ = state
                .secrets
                .set(&crate::storage::secrets::passphrase_account(&host_id), pp);
        }
    }

    Ok(info)
}

/// 决定这次连接用的主机 id（也就是「连接配置」的身份）。
///
/// - **要保存主机**：请求里带了 id 就用它（覆盖已有主机），没带就现生成一个。
/// - **不保存**：只有请求里带了才用。
///
/// 提前定下来的原因：前端要靠它从加密凭据库里取回密码，断线之后才连得上。
/// 晚一点定下来，前端就永远拿不到 —— 手动新建的主机尤其明显（密码存进了
/// 凭据库，却没人知道它存在哪把钥匙下）。
fn resolve_host_id(request: &ConnectRequest) -> Option<String> {
    let requested = request
        .host_id
        .as_ref()
        .filter(|id| !id.trim().is_empty())
        .cloned();
    if request.save.unwrap_or(false) {
        Some(requested.unwrap_or_else(|| uuid::Uuid::now_v7().to_string()))
    } else {
        requested
    }
}

/// 连通性测试：完成握手与认证后立即断开，不打开 shell。
#[tauri::command]
pub async fn session_test(
    state: State<'_, AppState>,
    window: tauri::Window,
    request: ConnectRequest,
) -> AppResult<TestResult> {
    let started = Instant::now();

    let credential = match resolve_credential(&request, &state.secrets) {
        Ok(c) => c,
        Err(e) => {
            return Ok(TestResult {
                ok: false,
                latency_ms: None,
                error: Some(e.payload()),
            })
        }
    };

    let host = request.host.trim().to_string();
    let username = request.username.trim().to_string();
    let port = request.port.unwrap_or(22);
    let label = format!("{username}@{host}");

    match client::connect(
        &state.app,
        client::ConnectParams {
            host: &host,
            port,
            username: &username,
            credential: &credential,
            label,
            auth_type: request.auth_type,
            // 测试用的会话不会注册进注册表，归属给谁无所谓 —— 但也得有个标签。
            window_label: window.label().to_string(),
            host_id: None,
        },
    )
    .await
    {
        Ok(session) => {
            let latency = started.elapsed().as_millis() as u64;
            session.shutdown().await;
            Ok(TestResult {
                ok: true,
                latency_ms: Some(latency),
                error: None,
            })
        }
        Err(e) => Ok(TestResult {
            ok: false,
            latency_ms: None,
            error: Some(e.payload()),
        }),
    }
}

/// 写入终端输入。
#[tauri::command]
pub async fn session_write(
    state: State<'_, AppState>,
    session_id: String,
    data: String,
) -> AppResult<()> {
    let session = state
        .sessions
        .get(&session_id)
        .ok_or_else(|| AppError::SessionGone("会话不存在或已关闭".into()))?;
    session.write(data.as_bytes()).await
}

/// 调整终端尺寸。
#[tauri::command]
pub async fn session_resize(
    state: State<'_, AppState>,
    session_id: String,
    cols: u32,
    rows: u32,
) -> AppResult<()> {
    let session = state
        .sessions
        .get(&session_id)
        .ok_or_else(|| AppError::SessionGone("会话不存在或已关闭".into()))?;
    session.resize(cols, rows).await
}

/// 前端挂载终端后调用，补发缓存的输出。
#[tauri::command]
pub async fn session_attach(state: State<'_, AppState>, session_id: String) -> AppResult<()> {
    let session = state
        .sessions
        .get(&session_id)
        .ok_or_else(|| AppError::SessionGone("会话不存在或已关闭".into()))?;
    session.attach(&state.app).await;
    Ok(())
}

/// 克隆会话：以现有连接为模板建立一个新会话。
///
/// 凭据在 Rust 侧复用，前端永远拿不到明文密码。
#[tauri::command]
pub async fn session_clone(
    state: State<'_, AppState>,
    session_id: String,
    label: Option<String>,
) -> AppResult<SessionInfo> {
    let source = state
        .sessions
        .get(&session_id)
        .ok_or_else(|| AppError::SessionGone("会话不存在或已关闭".into()))?;

    let session = client::clone_session(&state.app, &source, label.as_deref()).await?;
    let info = session.info.clone();
    state.sessions.insert(&info.session_id, session);
    Ok(info)
}

/// 断开连接并释放资源。
///
/// 这是缺陷 #2 / #9 的修复点：关闭标签页会真正断开 SSH 连接，
/// 而不是只把界面元素移除。
#[tauri::command]
pub async fn session_disconnect(state: State<'_, AppState>, session_id: String) -> AppResult<()> {
    if let Some(session) = state.sessions.remove(&session_id) {
        session.shutdown().await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(save: bool, host_id: Option<&str>) -> ConnectRequest {
        ConnectRequest {
            label: None,
            group: None,
            host: "10.0.0.1".into(),
            port: Some(22),
            username: "root".into(),
            auth_type: AuthType::Password,
            password: None,
            private_key_path: None,
            passphrase: None,
            save: Some(save),
            host_id: host_id.map(str::to_string),
        }
    }

    /// 缺陷 #43 的回归测试：保存主机时即便请求里没带 id，也必须有一个 ——
    /// 前端要靠它从加密凭据库里取回密码。
    #[test]
    fn saving_without_host_id_generates_one() {
        let id = resolve_host_id(&request(true, None)).expect("保存主机时必须有 id");
        assert!(!id.trim().is_empty());
    }

    /// 保存时带了 id 就用它 —— 那是「覆盖已有主机」。
    #[test]
    fn saving_with_host_id_keeps_it() {
        assert_eq!(
            resolve_host_id(&request(true, Some("h1"))).as_deref(),
            Some("h1")
        );
    }

    /// 不保存主机时，没带 id 就没有 id —— 不凭空造一个。
    #[test]
    fn not_saving_without_host_id_has_none() {
        assert!(resolve_host_id(&request(false, None)).is_none());
    }

    /// 不保存主机但带了 id（比如复用已有主机的凭据）→ 照用。
    #[test]
    fn not_saving_with_host_id_keeps_it() {
        assert_eq!(
            resolve_host_id(&request(false, Some("h1"))).as_deref(),
            Some("h1")
        );
    }

    /// 空字符串不算 id —— 不能拿它当钥匙。
    #[test]
    fn blank_host_id_is_ignored() {
        assert!(resolve_host_id(&request(false, Some("   "))).is_none());
        let generated = resolve_host_id(&request(true, Some("  "))).expect("保存时要生成一个");
        assert!(!generated.trim().is_empty());
    }
}
