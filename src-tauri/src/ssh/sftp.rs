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

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use russh_sftp::client::error::Error as SftpError;
use russh_sftp::client::SftpSession;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::error::{AppError, AppResult};
use crate::types::{RemoteFile, TransferDirection, TransferProgress};

/// 进度事件节流间隔：上传大文件时不该每个分块都发一次 IPC。
const PROGRESS_INTERVAL: Duration = Duration::from_millis(80);

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

/// 一次上传里要传的一个文件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadFile {
    /// 相对上传根的路径。单个文件上传时就是文件名。
    pub relative: String,
    /// 本地绝对路径。
    pub local: PathBuf,
    /// 字节数。
    pub size: u64,
}

/// 一次上传的完整计划（本地扫描结果）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UploadPlan {
    /// 需要在远端创建的目录（相对路径，父目录排在子目录之前）。
    pub dirs: Vec<String>,
    /// 待上传的文件。
    pub files: Vec<UploadFile>,
    /// 全部文件总字节数。
    pub total: u64,
}

/// 扫描本地路径，生成上传计划。
///
/// - 普通文件 → 单文件上传
/// - 目录 → 递归展开，远端目录结构一并创建
///
/// 符号链接一律跳过：既避免成环，也避免把本地目录结构意外暴露到远端。
pub fn plan_upload(root: &Path) -> AppResult<UploadPlan> {
    let meta = std::fs::metadata(root)?;
    let mut plan = UploadPlan::default();

    if meta.is_file() {
        let name = root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .ok_or_else(|| {
                AppError::InvalidInput(format!("无法解析本地文件名: {}", root.display()))
            })?;
        plan.total = meta.len();
        plan.files.push(UploadFile {
            relative: name,
            local: root.to_path_buf(),
            size: meta.len(),
        });
        return Ok(plan);
    }

    if !meta.is_dir() {
        return Err(AppError::InvalidInput(format!(
            "{} 不是普通文件或目录",
            root.display()
        )));
    }

    // 目录上传时远端也会建出同名目录（与 `scp -r` 一致）。
    let root_name = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if !root_name.is_empty() {
        plan.dirs.push(root_name.clone());
    }

    // 迭代式遍历，避免深目录爆栈。
    let mut stack: Vec<(PathBuf, String)> = vec![(root.to_path_buf(), root_name.clone())];
    while let Some((dir, prefix)) = stack.pop() {
        let mut entries: Vec<_> = std::fs::read_dir(&dir)
            .map_err(|e| AppError::InvalidInput(format!("无法读取目录 {}: {e}", dir.display())))?
            .filter_map(|e| e.ok())
            .collect();
        entries.sort_by_key(|e| e.file_name());

        for entry in entries {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            let Ok(link_meta) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if link_meta.file_type().is_symlink() {
                continue;
            }

            let relative = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };

            if link_meta.is_dir() {
                plan.dirs.push(relative.clone());
                stack.push((path, relative));
            } else if link_meta.is_file() {
                plan.total += link_meta.len();
                plan.files.push(UploadFile {
                    relative,
                    local: path,
                    size: link_meta.len(),
                });
            }
        }
    }

    // 父目录是子目录的前缀，字典序天然保证父目录排在前面，
    // 因此按字典序排序后就可以直接逐个创建。
    plan.dirs.sort();
    plan.dirs.dedup();
    plan.files.sort_by(|a, b| a.relative.cmp(&b.relative));
    Ok(plan)
}

/// 上传方向的进度事件。
fn emit_upload_progress(
    app: &tauri::AppHandle,
    transfer_id: &str,
    session_id: &str,
    file_name: &str,
    transferred: u64,
    total: u64,
) {
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
            file_name: file_name.to_string(),
            transferred,
            total,
            percent,
            direction: TransferDirection::Upload,
        },
    );
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

    /// 上传本地文件或目录到远端目录。返回远端根路径。
    ///
    /// 目录会**递归上传**：远端目录结构一并创建，符号链接跳过（避免成环）。
    /// 进度按整个上传的累计字节计算，因此界面上只有一条进度条。
    pub async fn upload(
        &self,
        local_path: &str,
        remote_dir: &str,
        transfer_id: &str,
        session_id: &str,
        app: &tauri::AppHandle,
    ) -> AppResult<String> {
        let root = Path::new(local_path);
        let plan = plan_upload(root)?;
        let root_name = root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();

        // 先把远端目录结构建出来（已存在时忽略错误）。
        for dir in &plan.dirs {
            let remote = join_remote(remote_dir, dir);
            let _ = self.session.create_dir(remote.as_str()).await;
        }

        let mut transferred: u64 = 0;
        let mut last_emit = Instant::now() - PROGRESS_INTERVAL;
        let mut current_name = root_name.clone();

        // 首个事件：界面立刻出现这条传输记录。
        emit_upload_progress(
            app,
            transfer_id,
            session_id,
            &current_name,
            transferred,
            plan.total,
        );

        for file in &plan.files {
            current_name = file.relative.clone();
            // 缺陷 #3 的修复：目标路径在这里由后端拼接，前端只给目录。
            let remote_path = join_remote(remote_dir, &file.relative);

            let mut local = tokio::fs::File::open(&file.local).await?;
            let mut remote = self
                .session
                .create(remote_path.as_str())
                .await
                .map_err(|e| map_sftp_error(&e))?;

            let mut buf = vec![0u8; 64 * 1024];
            loop {
                let n = local.read(&mut buf).await?;
                if n == 0 {
                    break;
                }
                remote.write_all(&buf[..n]).await?;
                transferred += n as u64;

                if last_emit.elapsed() >= PROGRESS_INTERVAL {
                    last_emit = Instant::now();
                    emit_upload_progress(
                        app,
                        transfer_id,
                        session_id,
                        &current_name,
                        transferred,
                        plan.total,
                    );
                }
            }

            remote.flush().await.ok();
            let _ = remote.close().await;
        }

        // 收尾：保证界面收到 100%。
        emit_upload_progress(
            app,
            transfer_id,
            session_id,
            &current_name,
            plan.total,
            plan.total,
        );

        Ok(join_remote(remote_dir, &root_name))
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

    /// 测试用的临时目录。
    fn temp_root(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static SEQ: AtomicU32 = AtomicU32::new(0);
        let n = SEQ.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("ssh7-upload-test-{tag}-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn plans_a_single_file() {
        let root = temp_root("single");
        let file = root.join("a.txt");
        std::fs::write(&file, b"hello").unwrap();

        let plan = plan_upload(&file).unwrap();
        assert_eq!(plan.files.len(), 1);
        assert_eq!(plan.files[0].relative, "a.txt");
        assert_eq!(plan.files[0].size, 5);
        assert_eq!(plan.total, 5);
        assert!(plan.dirs.is_empty());

        std::fs::remove_dir_all(&root).ok();
    }

    /// 目录上传会递归展开，并把目录结构一并建出来。
    #[test]
    fn plans_a_directory_recursively() {
        let base = temp_root("dir");
        let root = base.join("proj");
        std::fs::create_dir_all(root.join("sub/deep")).unwrap();
        std::fs::write(root.join("top.txt"), b"12").unwrap();
        std::fs::write(root.join("sub/mid.txt"), b"345").unwrap();
        std::fs::write(root.join("sub/deep/low.txt"), b"6789").unwrap();

        let plan = plan_upload(&root).unwrap();
        assert_eq!(plan.total, 2 + 3 + 4);

        let files: Vec<&str> = plan.files.iter().map(|f| f.relative.as_str()).collect();
        assert_eq!(
            files,
            vec!["proj/sub/deep/low.txt", "proj/sub/mid.txt", "proj/top.txt"]
        );
        assert_eq!(plan.dirs, vec!["proj", "proj/sub", "proj/sub/deep"]);

        // 父目录必须排在子目录前面，否则远端 create_dir 会失败。
        let parent = plan.dirs.iter().position(|d| d == "proj/sub").unwrap();
        let child = plan.dirs.iter().position(|d| d == "proj/sub/deep").unwrap();
        assert!(parent < child);

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn missing_path_is_rejected() {
        let missing = std::env::temp_dir().join("ssh7-does-not-exist-xyz");
        assert!(plan_upload(&missing).is_err());
    }
}
