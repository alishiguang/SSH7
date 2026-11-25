//! SSH7 —— 用 Rust 重写的跨平台 SSH 客户端。
//!
//! 架构分层：
//!
//! ```text
//! 前端 (React + xterm.js)
//!   │ invoke / listen
//!   ▼
//! commands/          IPC 边界：参数校验、DTO 映射、错误映射
//!   ▼
//! ssh/ storage/ state/    Rust 核心
//! ```
//!
//! `commands` 是唯一的跨层入口，保证前端永远拿不到 `russh` 的内部类型。

pub mod commands;
pub mod error;
pub mod ssh;
pub mod state;
pub mod storage;
pub mod types;

use tauri::Manager;

use crate::commands::AppState;

/// 应用入口。
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("SSH7_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        // 窗口关掉了，它自己的会话得跟着断 —— 不然连接一直挂在后台。
        //
        // 注册在 Builder 上而不是 App 上：这样**运行时新建的窗口**
        // （双击主机开出来的那种）也归它管。
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Destroyed = event {
                let label = window.label().to_string();
                let handle = window.app_handle().clone();
                tauri::async_runtime::spawn(async move {
                    let state = handle.state::<AppState>();
                    state.windows.forget(&label);
                    state.sessions.shutdown_window(&label).await;
                });
            }
        })
        .setup(|app| {
            let config_dir = app.path().app_config_dir()?;
            // 标识符改了（账号改名），数据目录也跟着换了 —— 旧目录还在就搬过来，
            // 否则用户的主机列表和凭据库会「凭空消失」。
            match crate::storage::legacy_dir::migrate_legacy_config_dir(&config_dir) {
                Ok(crate::storage::legacy_dir::Migration::Moved { from }) => {
                    tracing::info!("旧配置目录已迁移到新位置：{from}");
                }
                Ok(_) => {}
                Err(err) => tracing::warn!("旧配置目录迁移失败：{err}"),
            }
            std::fs::create_dir_all(&config_dir)?;
            let state = AppState::new(app.handle().clone(), config_dir);
            // 后端只剩一种了，旧版本留下的偏好文件没有意义 —— 顺手清掉。
            state.secrets.remove_legacy_prefs();
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::host::host_list,
            commands::host::host_save,
            commands::host::host_delete,
            commands::host::host_group_rename,
            commands::host::host_group_remove,
            commands::host::host_group_create,
            commands::host::host_groups_reorder,
            commands::host::host_move_to_group,
            commands::session::session_connect,
            commands::session::session_test,
            commands::session::session_write,
            commands::session::session_resize,
            commands::session::session_attach,
            commands::session::session_clone,
            commands::session::session_disconnect,
            commands::window::session_open_in_window,
            commands::window::window_boot,
            commands::sftp::sftp_list,
            commands::sftp::sftp_upload,
            commands::sftp::sftp_download,
            commands::sftp::sftp_delete,
            commands::sftp::sftp_mkdir,
            commands::sftp::sftp_pick_files,
            commands::sftp::sftp_pick_directory,
            commands::sftp::sftp_pick_upload_folder,
        ])
        .run(tauri::generate_context!())
        .expect("启动 SSH7 失败");
}
