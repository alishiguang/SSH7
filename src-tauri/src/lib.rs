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
        .setup(|app| {
            let config_dir = app.path().app_config_dir()?;
            std::fs::create_dir_all(&config_dir)?;
            let state = AppState::new(app.handle().clone(), config_dir);
            // 首次运行把当前凭据后端落盘，让「用的是哪个后端」在磁盘上显式可查。
            state.secrets.ensure_backend()?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::host::host_list,
            commands::host::host_save,
            commands::host::host_delete,
            commands::credentials::credential_store_info,
            commands::credentials::credential_store_set,
            commands::session::session_connect,
            commands::session::session_test,
            commands::session::session_write,
            commands::session::session_resize,
            commands::session::session_attach,
            commands::session::session_clone,
            commands::session::session_disconnect,
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
