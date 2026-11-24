//! 已保存主机的命令。
//!
//! 安全要点：**秘密（密码 / 私钥口令）只写入凭据库**，
//! `hosts.json` 里只保存「是否已保存秘密」的布尔标记（缺陷 #1）。

use tauri::State;

use crate::commands::AppState;
use crate::error::{AppError, AppResult};
use crate::storage::hosts::SavedHost;
use crate::storage::keyring;
use crate::storage::secrets::SecretStore;
use crate::types::{HostListDto, HostSaveRequest, SavedHostDto};

/// 主机列表 + 分组清单。
fn to_list(hosts: Vec<SavedHost>, groups: Vec<String>) -> HostListDto {
    HostListDto {
        hosts: hosts.into_iter().map(to_dto).collect(),
        groups,
    }
}

/// 读取全部已保存主机与分组清单。
#[tauri::command]
pub fn host_list(state: State<'_, AppState>) -> AppResult<HostListDto> {
    let hosts = state.hosts.load()?;
    let groups = state.hosts.groups()?;
    Ok(to_list(hosts, groups))
}

fn to_dto(h: SavedHost) -> SavedHostDto {
    SavedHostDto {
        id: h.id,
        label: h.label,
        group: h.group,
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
        group: dto.group.clone(),
        host: dto.host.clone(),
        port: dto.port,
        username: dto.username.clone(),
        auth_type: dto.auth_type,
        private_key_path: dto.private_key_path.clone(),
        has_password: dto.has_password,
        has_passphrase: dto.has_passphrase,
    }
}

/// 新增或更新主机。秘密（若有）写入凭据库。
#[tauri::command]
pub fn host_save(state: State<'_, AppState>, request: HostSaveRequest) -> AppResult<HostListDto> {
    let mut host = from_dto(&request.host);

    // 密码
    if let Some(pw) = request.password.as_ref().filter(|p| !p.is_empty()) {
        state
            .secrets
            .set(&keyring::password_account(&host.id), pw)?;
        host.has_password = true;
    } else if request.password.as_ref().is_some() {
        // 显式传空串表示清除密码。
        let _ = state.secrets.delete(&keyring::password_account(&host.id));
        host.has_password = false;
    }

    // 私钥口令
    if let Some(pp) = request.passphrase.as_ref().filter(|p| !p.is_empty()) {
        state
            .secrets
            .set(&keyring::passphrase_account(&host.id), pp)?;
        host.has_passphrase = true;
    } else if request.passphrase.as_ref().is_some() {
        let _ = state.secrets.delete(&keyring::passphrase_account(&host.id));
        host.has_passphrase = false;
    }

    let (hosts, groups) = state.hosts.upsert(&host)?;
    Ok(to_list(hosts, groups))
}

/// 删除主机，同时清理凭据库条目。
#[tauri::command]
pub fn host_delete(state: State<'_, AppState>, host_id: String) -> AppResult<HostListDto> {
    let _ = state.secrets.delete(&keyring::password_account(&host_id));
    let _ = state.secrets.delete(&keyring::passphrase_account(&host_id));
    let (hosts, groups) = state.hosts.remove(&host_id)?;
    Ok(to_list(hosts, groups))
}

/// 新建分组。**可以是空分组** —— 用户先把「生产 / 测试」建好，
/// 往里放机器时直接选，不用等第一台机器保存了分组名才出现。
#[tauri::command]
pub fn host_group_create(state: State<'_, AppState>, name: String) -> AppResult<HostListDto> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::InvalidInput("分组名不能为空".into()));
    }
    let (hosts, groups) = state.hosts.create_group(name)?;
    Ok(to_list(hosts, groups))
}

/// 重命名分组：组里的主机一起改到新名字。
///
/// 新名字跟已有分组重名时两组自然合并 —— 这正是用户想要的效果，
/// 所以这里不做「重名报错」。
#[tauri::command]
pub fn host_group_rename(
    state: State<'_, AppState>,
    old_name: String,
    new_name: String,
) -> AppResult<HostListDto> {
    let old_name = old_name.trim();
    let new_name = new_name.trim();
    if old_name.is_empty() {
        return Err(AppError::InvalidInput("未分组不能改名".into()));
    }
    if new_name.is_empty() {
        return Err(AppError::InvalidInput("分组名不能为空".into()));
    }
    let (hosts, groups) = state.hosts.rename_group(old_name, new_name)?;
    Ok(to_list(hosts, groups))
}

/// 删除分组：组里的主机回到「未分组」，**主机本身不删**。
#[tauri::command]
pub fn host_group_remove(state: State<'_, AppState>, name: String) -> AppResult<HostListDto> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::InvalidInput("未分组不能删除".into()));
    }
    let (hosts, groups) = state.hosts.remove_group(name)?;
    Ok(to_list(hosts, groups))
}

/// 读取某主机保存的密码。**只供 Rust 内部连接流程使用**，
/// 不暴露给前端 —— 前端永远拿不到明文。
pub(crate) fn load_password(store: &SecretStore, host_id: &str) -> AppResult<Option<String>> {
    store.get(&keyring::password_account(host_id))
}

/// 读取某主机保存的私钥口令。
pub(crate) fn load_passphrase(store: &SecretStore, host_id: &str) -> AppResult<Option<String>> {
    store.get(&keyring::passphrase_account(host_id))
}
