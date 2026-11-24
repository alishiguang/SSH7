//! 已保存主机的命令。
//!
//! 安全要点：**秘密（密码 / 私钥口令）只写入凭据库**，
//! `hosts.json` 里只保存「是否已保存秘密」的布尔标记（缺陷 #1）。

use tauri::State;

use crate::commands::AppState;
use crate::error::{AppError, AppResult};
use crate::storage::hosts::{HostsStore, SavedHost};
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
    save_host(&state.hosts, &state.secrets, request)
}

/// 保存主机 + 秘密。**不依赖 Tauri**，方便单独测。
///
/// 秘密只写进凭据库；`hosts.json` 里只留「有没有」的标记（缺陷 #1）。
pub fn save_host(
    hosts: &HostsStore,
    secrets: &SecretStore,
    request: HostSaveRequest,
) -> AppResult<HostListDto> {
    let mut host = from_dto(&request.host);

    // 空 id 表示「这是台新机器」—— id 由后端分配，前端不用自己生成。
    if host.id.trim().is_empty() {
        host.id = uuid::Uuid::now_v7().to_string();
    }

    // 密码
    if let Some(pw) = request.password.as_ref().filter(|p| !p.is_empty()) {
        secrets.set(&keyring::password_account(&host.id), pw)?;
        host.has_password = true;
    } else if request.password.as_ref().is_some() {
        // 显式传空串表示清除密码。
        let _ = secrets.delete(&keyring::password_account(&host.id));
        host.has_password = false;
    }

    // 私钥口令
    if let Some(pp) = request.passphrase.as_ref().filter(|p| !p.is_empty()) {
        secrets.set(&keyring::passphrase_account(&host.id), pp)?;
        host.has_passphrase = true;
    } else if request.passphrase.as_ref().is_some() {
        let _ = secrets.delete(&keyring::passphrase_account(&host.id));
        host.has_passphrase = false;
    }

    let (saved, groups) = hosts.upsert(&host)?;
    Ok(to_list(saved, groups))
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

/// 重排分组顺序。`order` 是新的分组名顺序。
#[tauri::command]
pub fn host_groups_reorder(
    state: State<'_, AppState>,
    order: Vec<String>,
) -> AppResult<HostListDto> {
    let (hosts, groups) = state.hosts.reorder_groups(&order)?;
    Ok(to_list(hosts, groups))
}

/// 把主机挪到某个分组。`group` 为空表示回到「未分组」。
#[tauri::command]
pub fn host_move_to_group(
    state: State<'_, AppState>,
    host_id: String,
    group: Option<String>,
) -> AppResult<HostListDto> {
    let (hosts, groups) = state.hosts.set_group(&host_id, group.as_deref())?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::secrets::SecretBackend;
    use crate::types::AuthType;

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("ssh7-host-save-{tag}-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 建一套不碰系统钥匙串的存储（本地加密文件）。
    fn stores(tag: &str) -> (HostsStore, SecretStore, std::path::PathBuf) {
        let dir = temp_dir(tag);
        let hosts = HostsStore::new(&dir);
        let secrets = SecretStore::new(&dir);
        // 明确切到本地加密库 —— 测试不该去动用户的系统钥匙串。
        secrets
            .set_backend(SecretBackend::Vault, &[])
            .expect("切到本地加密库");
        (hosts, secrets, dir)
    }

    fn dto(id: &str) -> SavedHostDto {
        SavedHostDto {
            id: id.to_string(),
            label: "测试机".into(),
            group: None,
            host: "10.0.0.1".into(),
            port: 22,
            username: "root".into(),
            auth_type: AuthType::Password,
            private_key_path: None,
            has_password: false,
            has_passphrase: false,
        }
    }

    /// 「仅保存」的新建路径：前端不生成 id，后端分配。
    #[test]
    fn empty_id_gets_a_generated_one() {
        let (hosts, secrets, dir) = stores("new-id");
        let list = save_host(
            &hosts,
            &secrets,
            HostSaveRequest {
                host: dto(""),
                password: Some("pw".into()),
                passphrase: None,
            },
        )
        .expect("保存失败");

        let saved = &list.hosts[0];
        assert!(!saved.id.is_empty(), "空 id 必须由后端分配");
        assert!(
            uuid::Uuid::parse_str(&saved.id).is_ok(),
            "分配的 id 应该是 UUID：{}",
            saved.id
        );
        // 列表里还拿不到明文密码。
        assert!(saved.has_password);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 已有的 id 不被覆盖 —— 否则「编辑」会变成「新建」。
    #[test]
    fn existing_id_is_kept() {
        let (hosts, secrets, dir) = stores("keep-id");
        let list = save_host(
            &hosts,
            &secrets,
            HostSaveRequest {
                host: dto("fixed-id"),
                password: None,
                passphrase: None,
            },
        )
        .expect("保存失败");
        assert_eq!(list.hosts[0].id, "fixed-id");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 秘密进凭据库，`hosts.json` 里一个字都不能有（缺陷 #1）。
    #[test]
    fn password_never_reaches_hosts_json() {
        let (hosts, secrets, dir) = stores("secret");
        let list = save_host(
            &hosts,
            &secrets,
            HostSaveRequest {
                host: dto(""),
                password: Some("hunter2-should-not-leak".into()),
                passphrase: None,
            },
        )
        .expect("保存失败");

        let raw = std::fs::read_to_string(dir.join("hosts.json")).expect("读 hosts.json");
        assert!(
            !raw.contains("hunter2-should-not-leak"),
            "hosts.json 里不该出现明文密码：{raw}"
        );
        assert!(raw.contains("hasPassword"), "只该留下「有没有」的标记");

        // 凭据库里能读回来。
        let id = &list.hosts[0].id;
        let stored = secrets
            .get(&keyring::password_account(id))
            .expect("读凭据")
            .expect("凭据库里应该有密码");
        assert_eq!(stored, "hunter2-should-not-leak");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 密码留空 = 保持原来那个，不是清除（编辑主机时不回显密码）。
    #[test]
    fn omitting_password_keeps_the_stored_one() {
        let (hosts, secrets, dir) = stores("keep-pw");
        let first = save_host(
            &hosts,
            &secrets,
            HostSaveRequest {
                host: dto("fixed-id"),
                password: Some("keep-me".into()),
                passphrase: None,
            },
        )
        .expect("首次保存失败");

        // 第二次不传密码 —— 应该原样保留。
        let mut edited = dto("fixed-id");
        edited.has_password = first.hosts[0].has_password;
        let list = save_host(
            &hosts,
            &secrets,
            HostSaveRequest {
                host: edited,
                password: None,
                passphrase: None,
            },
        )
        .expect("二次保存失败");

        assert!(list.hosts[0].has_password, "标记应该还在");
        let stored = secrets
            .get(&keyring::password_account("fixed-id"))
            .expect("读凭据")
            .expect("密码不该被清掉");
        assert_eq!(stored, "keep-me");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
