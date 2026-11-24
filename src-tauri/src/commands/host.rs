//! 已保存主机的命令。
//!
//! 安全要点：**秘密（密码 / 私钥口令）只写入系统钥匙串**，
//! `hosts.json` 里只保存「是否已保存秘密」的布尔标记（缺陷 #1）。

use tauri::State;

use crate::commands::AppState;
use crate::error::AppResult;
use crate::storage::hosts::SavedHost;
use crate::storage::keyring;
use crate::types::{HostSaveRequest, SavedHostDto};

/// 读取全部已保存主机。
#[tauri::command]
pub fn host_list(state: State<'_, AppState>) -> AppResult<Vec<SavedHostDto>> {
    let hosts = state.hosts.load()?;
    Ok(hosts.into_iter().map(to_dto).collect())
}

fn to_dto(h: SavedHost) -> SavedHostDto {
    SavedHostDto {
        id: h.id,
        label: h.label,
        host: h.host,
        port: h.port,
        username: h.username,
        auth_type: h.auth_type,
        private_key_path: h.private_key_path,
        has_password: h.has_password,
        has_passphrase: h.has_passphrase,
    }
}

fn from_dto(dto: &SavedHostDto) -> SavedHost {
    SavedHost {
        id: dto.id.clone(),
        label: dto.label.clone(),
        host: dto.host.clone(),
        port: dto.port,
        username: dto.username.clone(),
        auth_type: dto.auth_type,
        private_key_path: dto.private_key_path.clone(),
        has_password: dto.has_password,
        has_passphrase: dto.has_passphrase,
    }
}

/// 新增或更新主机。秘密（若有）写入钥匙串。
#[tauri::command]
pub fn host_save(
    state: State<'_, AppState>,
    request: HostSaveRequest,
) -> AppResult<Vec<SavedHostDto>> {
    let mut host = from_dto(&request.host);

    // 密码
    if let Some(pw) = request.password.as_ref().filter(|p| !p.is_empty()) {
        keyring::set(&keyring::password_account(&host.id), pw)?;
        host.has_password = true;
    } else if request.password.as_ref().is_some() {
        // 显式传空串表示清除密码。
        let _ = keyring::delete(&keyring::password_account(&host.id));
        host.has_password = false;
    }

    // 私钥口令
    if let Some(pp) = request.passphrase.as_ref().filter(|p| !p.is_empty()) {
        keyring::set(&keyring::passphrase_account(&host.id), pp)?;
        host.has_passphrase = true;
    } else if request.passphrase.as_ref().is_some() {
        let _ = keyring::delete(&keyring::passphrase_account(&host.id));
        host.has_passphrase = false;
    }

    let hosts = state.hosts.upsert(&host)?;
    Ok(hosts.into_iter().map(to_dto).collect())
}

/// 删除主机，同时清理钥匙串条目。
#[tauri::command]
pub fn host_delete(state: State<'_, AppState>, host_id: String) -> AppResult<Vec<SavedHostDto>> {
    let _ = keyring::delete(&keyring::password_account(&host_id));
    let _ = keyring::delete(&keyring::passphrase_account(&host_id));
    let hosts = state.hosts.remove(&host_id)?;
    Ok(hosts.into_iter().map(to_dto).collect())
}

/// 读取某主机在钥匙串中保存的密码。**只供 Rust 内部连接流程使用**，
/// 不暴露给前端 —— 前端永远拿不到明文。
pub(crate) fn load_password(host_id: &str) -> AppResult<Option<String>> {
    keyring::get(&keyring::password_account(host_id))
}

/// 读取某主机在钥匙串中保存的私钥口令。
pub(crate) fn load_passphrase(host_id: &str) -> AppResult<Option<String>> {
    keyring::get(&keyring::passphrase_account(host_id))
}
