//! SFTP 文件操作。
//!
//! 相比旧版的关键改动：
//!
//! - **一个连接只开一个 SFTP channel 并复用**（缺陷 #4 / #5 / #17）。
//!   旧版每次 `list` / `upload` / `download` 都新建一个 channel，
//!   且错误路径不调用 `end()`，导致 channel 泄漏。
//! - **上传目标路径由后端拼接**（缺陷 #3）。旧版前端直接传目录路径，
//!   后端靠 `endsWith('/')` 判断要不要拼文件名，导致在子目录上传必然失败。
//! - **递归删除**（缺陷 #9）。旧版用 `rmdir`，非空目录必然失败。
//! - **进度按 transfer_id 关联**（缺陷 #6），不再用文件名匹配。

use std::sync::Arc;

use russh_sftp::client::error::Error as SftpError;
use russh_sftp::client::SftpSession;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::error::{AppError, AppResult};
use crate::types::{RemoteFile, TransferDirection, TransferProgress};

/// 远端路径拼接。语义明确，不依赖尾部斜杠。
pub fn join_remote(dir: &str, name: &str) -> String {
    let base = dir.trim_end_matches('/');
    if base.is_empty() {
        format!("/{name}")
    } else {
        format!("{base}/{name}")
    }
}

/// 远端路径的父目录。
pub fn parent_remote(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    match trimmed.rfind('/') {
        None | Some(0) => "/".to_string(),
        Some(i) => trimmed[..i].to_string(),
    }
}

/// SFTP 客户端封装。
pub struct SftpClient {
    session: SftpSession,
}

/// 把 SFTP 错误映射为结构化应用错误。
fn map_sftp_error(e: &SftpError) -> AppError {
    let text = format!("{e:?}");
    let lower = text.to_ascii_lowercase();
    if lower.contains("nosuchfile") || lower.contains("no such file") {
        AppError::NoSuchPath(text)
    } else if lower.contains("permission") || lower.contains("denied") {
        AppError::PermissionDenied(text)
    } else if lower.contains("failure") && lower.contains("exist") {
        AppError::PermissionDenied("远端已存在同名文件".into())
    } else {
        AppError::Internal(text)
    }
}

impl SftpClient {
    /// 打开一个新的 SFTP channel。
    pub async fn open(
        handle: Arc<tokio::sync::Mutex<russh::client::Handle<crate::ssh::client::ClientHandler>>>,
    ) -> AppResult<Arc<Self>> {
        let channel = {
            let guard = handle.lock().await;
            guard
                .channel_open_session()
                .await
                .map_err(|e| crate::error::map_ssh_error(&e))?
        };
        let stream = channel.into_stream();
        let session = SftpSession::new(stream)
            .await
            .map_err(|e| map_sftp_error(&e))?;
        Ok(Arc::new(Self { session }))
    }

    /// 关闭 channel。
    pub async fn close(&self) {
        let _ = self.session.close().await;
    }

    /// 列目录。
    pub async fn list(&self, path: &str) -> AppResult<Vec<RemoteFile>> {
        let dir = self
            .session
            .read_dir(path)
            .await
            .map_err(|e| map_sftp_error(&e))?;

        let mut out = Vec::new();
        for entry in dir {
            let meta = entry.metadata();
            out.push(RemoteFile {
                name: entry.file_name(),
                path: entry.path(),
                is_directory: meta.is_dir(),
                is_symlink: meta.is_symlink(),
                size: meta.size.unwrap_or(0),
                modified_at: meta.mtime.unwrap_or(0) as u64,
                permissions: meta.permissions,
            });
        }
        Ok(out)
    }

    /// 新建目录。
    pub async fn mkdir(&self, path: &str) -> AppResult<()> {
        self.session
            .create_dir(path)
            .await
            .map_err(|e| map_sftp_error(&e))
    }

    /// 上传本地文件到远端目录。返回远端完整路径。
    pub async fn upload(
        &self,
        local_path: &str,
        remote_dir: &str,
        transfer_id: &str,
        session_id: &str,
        app: &tauri::AppHandle,
    ) -> AppResult<String> {
        let file_name = std::path::Path::new(local_path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .ok_or_else(|| AppError::InvalidInput(format!("无法解析本地文件名: {local_path}")))?;

        // 缺陷 #3 的修复：目标路径在这里由后端拼接，前端只给目录。
        let remote_path = join_remote(remote_dir, &file_name);

        let total = crate::ssh::auth::ensure_readable(std::path::Path::new(local_path))?;

        let mut local = tokio::fs::File::open(local_path).await?;
        let mut remote = self
            .session
            .create(remote_path.as_str())
            .await
            .map_err(|e| map_sftp_error(&e))?;

        let mut buf = vec![0u8; 64 * 1024];
        let mut transferred: u64 = 0;

        loop {
            let n = local.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            remote.write_all(&buf[..n]).await?;
            transferred += n as u64;

            // 缺陷 #13 的修复：total 为 0 时不会出现 NaN。
            let percent = if total == 0 {
                100
            } else {
                ((transferred as f64 / total as f64) * 100.0)
                    .round()
                    .clamp(0.0, 100.0) as u8
            };

            crate::ssh::client::emit_transfer_progress(
                app,
                TransferProgress {
                    transfer_id: transfer_id.to_string(),
                    session_id: session_id.to_string(),
                    file_name: file_name.clone(),
                    transferred,
                    total,
                    percent,
                    direction: TransferDirection::Upload,
                },
            );
        }

        remote.flush().await.ok();
        let _ = remote.close().await;
        Ok(remote_path)
    }

    /// 下载远端文件到本地目录。返回本地完整路径。
    pub async fn download(
        &self,
        remote_path: &str,
        local_dir: &str,
        transfer_id: &str,
        session_id: &str,
        app: &tauri::AppHandle,
    ) -> AppResult<String> {
        let file_name = std::path::Path::new(remote_path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .ok_or_else(|| AppError::InvalidInput(format!("无法解析远端文件名: {remote_path}")))?;

        let local_path = std::path::Path::new(local_dir).join(&file_name);

        let mut remote = self
            .session
            .open(remote_path)
            .await
            .map_err(|e| map_sftp_error(&e))?;
        let mut local = tokio::fs::File::create(&local_path).await?;

        let total = remote
            .metadata()
            .await
            .map_err(|e| map_sftp_error(&e))?
            .size
            .unwrap_or(0);

        let mut buf = vec![0u8; 64 * 1024];
        let mut transferred: u64 = 0;

        loop {
            let n = remote.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            local.write_all(&buf[..n]).await?;
            transferred += n as u64;

            let percent = if total == 0 {
                100
            } else {
                ((transferred as f64 / total as f64) * 100.0)
                    .round()
                    .clamp(0.0, 100.0) as u8
            };

            crate::ssh::client::emit_transfer_progress(
                app,
                TransferProgress {
                    transfer_id: transfer_id.to_string(),
                    session_id: session_id.to_string(),
                    file_name: file_name.clone(),
                    transferred,
                    total,
                    percent,
                    direction: TransferDirection::Download,
                },
            );
        }

        local.flush().await.ok();
        Ok(local_path.to_string_lossy().into_owned())
    }

    /// 删除远端路径。`recursive` 为真时递归删除目录内容。
    ///
    /// 用显式栈代替递归，避免 async fn 递归装箱。
    pub async fn remove(&self, path: &str, recursive: bool) -> AppResult<()> {
        let mut stack = vec![path.to_string()];
        while let Some(current) = stack.pop() {
            let meta = self
                .session
                .symlink_metadata(current.as_str())
                .await
                .map_err(|e| map_sftp_error(&e))?;

            if meta.is_dir() {
                if recursive {
                    let dir = self
                        .session
                        .read_dir(current.as_str())
                        .await
                        .map_err(|e| map_sftp_error(&e))?;
                    for entry in dir {
                        stack.push(entry.path());
                    }
                }
                self.session
                    .remove_dir(current.as_str())
                    .await
                    .map_err(|e| map_sftp_error(&e))?;
            } else {
                self.session
                    .remove_file(current.as_str())
                    .await
                    .map_err(|e| map_sftp_error(&e))?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 缺陷 #3 的回归测试：任意目录下的上传目标路径都能正确拼接。
    #[test]
    fn joins_remote_paths() {
        assert_eq!(join_remote("/", "a.txt"), "/a.txt");
        assert_eq!(join_remote("/root", "a.txt"), "/root/a.txt");
        assert_eq!(join_remote("/root/", "a.txt"), "/root/a.txt");
        assert_eq!(join_remote("/root//", "a.txt"), "/root/a.txt");
        assert_eq!(join_remote("", "a.txt"), "/a.txt");
        assert_eq!(join_remote("/a/b/c", "d.txt"), "/a/b/c/d.txt");
    }

    #[test]
    fn computes_parent_paths() {
        assert_eq!(parent_remote("/"), "/");
        assert_eq!(parent_remote("/root"), "/");
        assert_eq!(parent_remote("/root/a.txt"), "/root");
        assert_eq!(parent_remote("/a/b/c.txt"), "/a/b");
        assert_eq!(parent_remote("/a/b/"), "/a");
    }

    /// 缺陷 #6 / #13 的回归测试：百分比计算不会产生 NaN。
    #[test]
    fn percent_is_clamped_and_never_nan() {
        let calc = |t: u64, total: u64| -> u8 {
            if total == 0 {
                100
            } else {
                ((t as f64 / total as f64) * 100.0)
                    .round()
                    .clamp(0.0, 100.0) as u8
            }
        };
        assert_eq!(calc(0, 0), 100);
        assert_eq!(calc(50, 100), 50);
        assert_eq!(calc(100, 100), 100);
        assert_eq!(calc(200, 100), 100);
    }
}
