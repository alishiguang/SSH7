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
/// 2. 凭据库中为该 `host_id` 保存的值（复��已保存主机时走这条路径）。
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

    let session = client::connect(
        &state.app,
        &host,
        port,
        &username,
        &credential,
        label.clone(),
        request.auth_type,
    )
    .await?;

    let info = session.info.clone();
    state.sessions.insert(&info.session_id, session);

    // 保存主机与秘密（秘密进凭据库，hosts.json 不含明文）。
    if request.save.unwrap_or(false) {
        let host_id = request
            .host_id
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
                .set(&crate::storage::keyring::password_account(&host_id), pw);
        }
        if let Some(pp) = request.passphrase.as_ref().filter(|p| !p.is_empty()) {
            let _ = state
                .secrets
                .set(&crate::storage::keyring::passphrase_account(&host_id), pp);
        }
    }

    Ok(info)
}

/// 连通性测试：完成握手与认证后立即断开，不打开 shell。
#[tauri::command]
pub async fn session_test(
    state: State<'_, AppState>,
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
        &host,
        port,
        &username,
        &credential,
        label,
        request.auth_type,
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
