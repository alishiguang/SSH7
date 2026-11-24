//! SFTP 文件命令与原生文件选择对话框。

use tauri::State;
use tauri_plugin_dialog::DialogExt;

use crate::commands::AppState;
use crate::error::{AppError, AppResult};
use crate::types::RemoteFile;

/// 取出会话；会话不存在时返回明确错误而不是静默失败。
fn session_of(
    state: &State<'_, AppState>,
    session_id: &str,
) -> AppResult<std::sync::Arc<crate::ssh::client::Session>> {
    state
        .sessions
        .get(session_id)
        .ok_or_else(|| AppError::SessionGone("会话不存在或已关闭".into()))
}

/// 列目录。
#[tauri::command]
pub async fn sftp_list(
    state: State<'_, AppState>,
    session_id: String,
    path: String,
) -> AppResult<Vec<RemoteFile>> {
    let session = session_of(&state, &session_id)?;
    let sftp = session.sftp().await?;
    sftp.list(&path).await
}

/// 上传本地文件到远端目录。
///
/// `remote_dir` 是**目录**，文件名由后端从本地路径提取并拼接 —— 这是
/// 缺陷 #3 的修复：旧版依赖前端拼路径，在子目录上传必然失败。
///
/// `transfer_id` 由前端生成并传入，保证进度事件能对上界面上那一条
/// 传输记录（缺陷 #6 的修复：不再用文件名做标识）。
#[tauri::command]
pub async fn sftp_upload(
    state: State<'_, AppState>,
    session_id: String,
    local_path: String,
    remote_dir: String,
    transfer_id: String,
) -> AppResult<String> {
    let session = session_of(&state, &session_id)?;
    let sftp = session.sftp().await?;

    sftp.upload(
        &local_path,
        &remote_dir,
        &transfer_id,
        &session_id,
        &state.app,
    )
    .await
}

/// 下载远端文件到本地目录。
#[tauri::command]
pub async fn sftp_download(
    state: State<'_, AppState>,
    session_id: String,
    remote_path: String,
    local_dir: String,
    transfer_id: String,
) -> AppResult<String> {
    let session = session_of(&state, &session_id)?;
    let sftp = session.sftp().await?;

    sftp.download(
        &remote_path,
        &local_dir,
        &transfer_id,
        &session_id,
        &state.app,
    )
    .await
}

/// 删除远端路径。目录可递归删除（缺陷 #9 的修复）。
#[tauri::command]
pub async fn sftp_delete(
    state: State<'_, AppState>,
    session_id: String,
    remote_path: String,
    recursive: bool,
) -> AppResult<()> {
    let session = session_of(&state, &session_id)?;
    let sftp = session.sftp().await?;
    sftp.remove(&remote_path, recursive).await
}

/// 新建远端目录。
#[tauri::command]
pub async fn sftp_mkdir(
    state: State<'_, AppState>,
    session_id: String,
    remote_path: String,
) -> AppResult<()> {
    let session = session_of(&state, &session_id)?;
    let sftp = session.sftp().await?;
    sftp.mkdir(&remote_path).await
}

/// 弹出本地文件选择框（可多选）。
#[tauri::command]
pub async fn sftp_pick_files(app: tauri::AppHandle) -> AppResult<Vec<String>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title("选择要上传的文件")
        .pick_files(move |res| {
            let _ = tx.send(res);
        });

    let picked = rx
        .await
        .map_err(|_| AppError::Internal("文件选择被取消".into()))?;

    Ok(match picked {
        Some(files) => files
            .into_iter()
            .filter_map(|f| f.into_path().ok())
            .map(|p| p.to_string_lossy().into_owned())
            .collect(),
        None => Vec::new(),
    })
}

/// 弹出本地目录选择框。
#[tauri::command]
pub async fn sftp_pick_directory(app: tauri::AppHandle) -> AppResult<Option<String>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title("选择保存目录")
        .pick_folder(move |res| {
            let _ = tx.send(res);
        });

    let picked = rx
        .await
        .map_err(|_| AppError::Internal("目录选择被取消".into()))?;

    Ok(picked
        .and_then(|f| f.into_path().ok())
        .map(|p| p.to_string_lossy().into_owned()))
}
