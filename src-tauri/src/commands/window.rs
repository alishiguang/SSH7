//! 窗口命令：把一台机器开到一个窗口里。
//!
//! 「双击连接配置」想做的事，本质是**一台机器 ↔ 一扇窗**：
//!
//! - 这台机器还没开着 → 给它开一扇新窗；
//! - 已经开着 → 不用再开，把那扇窗叫到前面来；
//! - 开着好几扇 → 挑一扇叫到前面来。
//!
//! 前端的状态是每扇窗一份，所以「开着没有」这件事由 Rust 侧回答：
//! 会话注册表知道每个会话归哪扇窗，窗口登记表知道每扇窗是为谁开的。

use tauri::{Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};

use crate::commands::AppState;
use crate::error::{AppError, AppResult};
use crate::state::windows::WindowAssignment;
use crate::types::{
    event, FocusSessionEvent, OpenWindowAction, OpenWindowRequest, OpenWindowResult,
};

/// 新窗口的默认尺寸 —— 与主窗口一致（见 `tauri.conf.json`）。
const WINDOW_WIDTH: f64 = 1240.0;
const WINDOW_HEIGHT: f64 = 820.0;
const WINDOW_MIN_WIDTH: f64 = 900.0;
const WINDOW_MIN_HEIGHT: f64 = 580.0;

/// 找出这扇窗里已经开着（且还活着）的、指向同一台机器的会话。
fn live_session_in_window(state: &AppState, key: &str, window_label: &str) -> Option<String> {
    for id in state.sessions.ids() {
        let Some(s) = state.sessions.get(&id) else {
            continue;
        };
        if s.is_closed() {
            continue;
        }
        if s.window_label != window_label {
            continue;
        }
        if s.target_key() != key {
            continue;
        }
        return Some(s.info.session_id.clone());
    }
    None
}

/// 把某台机器开到一个窗口里。
///
/// 返回三种结果之一：
/// - `switch`：当前窗口里已经有这个会话，切标签就行；
/// - `focused`：别的窗口里开着，已经把那扇窗叫到前面；
/// - `created`：之前都没开着，新开了一扇窗。
#[tauri::command]
pub async fn session_open_in_window(
    state: State<'_, AppState>,
    window: tauri::Window,
    request: OpenWindowRequest,
) -> AppResult<OpenWindowResult> {
    let current = window.label().to_string();
    let assignment = WindowAssignment {
        host_id: request.host_id.clone(),
        host: request.host.clone(),
        port: request.port,
        username: request.username.clone(),
        label: request.label.clone(),
    };
    let key = assignment.key();

    // 1) 当前窗口里已经有这个会话 —— 切标签，别再开窗。
    if let Some(session_id) = live_session_in_window(&state, &key, &current) {
        return Ok(OpenWindowResult {
            action: OpenWindowAction::Switch,
            session_id: Some(session_id),
            window: Some(current),
        });
    }

    // 2) 别的窗口里开着 → 定位过去。
    //
    //    这里不只看「已经连上的会话」，也看「刚开、还没连上的窗」——
    //    否则连点两下就会开出两扇窗（第一扇还在握手的工夫，第二下已经
    //    找不到会话了）。
    let mut candidates = state.windows.windows_for(&key);
    // 已经建好的会话也补进来：主窗口里用普通方式连上的会话，
    // 并不在「窗口指派」里，但同样算「这台机器开着」。
    for id in state.sessions.ids() {
        let Some(s) = state.sessions.get(&id) else {
            continue;
        };
        if s.is_closed() || s.target_key() != key {
            continue;
        }
        if !candidates.contains(&s.window_label) {
            candidates.push(s.window_label.clone());
        }
    }

    for wlabel in candidates {
        if wlabel == current {
            continue;
        }
        // 窗口还在吗？
        let Some(w) = state.app.get_webview_window(&wlabel) else {
            // 窗没了，登记也清掉，免得越积越多。
            state.windows.forget(&wlabel);
            continue;
        };

        // 已经连上 → 让那扇窗切到对应的标签，再把它叫到前面。
        if let Some(session_id) = live_session_in_window(&state, &key, &wlabel) {
            let _ = w.set_focus();
            let _ = state.app.emit_to(
                tauri::EventTarget::Window {
                    label: wlabel.clone(),
                },
                event::FOCUS_SESSION,
                FocusSessionEvent {
                    session_id: session_id.clone(),
                },
            );
            return Ok(OpenWindowResult {
                action: OpenWindowAction::Focused,
                session_id: Some(session_id),
                window: Some(wlabel),
            });
        }

        // 还没连上（刚开的窗）→ 直接把它叫到前面。
        let _ = w.set_focus();
        return Ok(OpenWindowResult {
            action: OpenWindowAction::Focused,
            session_id: None,
            window: Some(wlabel),
        });
    }

    // 3) 都没开着 → 新开一扇。
    let label = format!("host-{}", uuid::Uuid::now_v7().simple());
    state.windows.assign(&label, assignment);

    let title = match request
        .label
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(name) => format!("SSH7 · {name}"),
        None => "SSH7".to_string(),
    };
    build_host_window(&state.app, &label, &title)?;

    Ok(OpenWindowResult {
        action: OpenWindowAction::Created,
        session_id: None,
        window: Some(label),
    })
}

/// 窗口启动时问一次：这扇窗是专门为哪台机器开的？
///
/// 新窗口靠它自动连上目标主机 —— 参数不经过 URL，也就不存在
/// 「URL 里的参数还没到，前端已经连上了」的竞态。
///
/// 顺便把**上一轮页面**遗留的会话断掉：前端刷新过一次，标签页就没了，
/// 但连接还挂在 Rust 侧（缺陷 #2 / #9 在多窗口下的延伸）。
#[tauri::command]
pub async fn window_boot(
    state: State<'_, AppState>,
    window: tauri::Window,
) -> AppResult<Option<WindowAssignment>> {
    let label = window.label().to_string();
    state.sessions.shutdown_window(&label).await;
    Ok(state.windows.assignment(&label))
}

/// 新建一扇窗口。外观与主窗口保持一致。
fn build_host_window(app: &tauri::AppHandle, label: &str, title: &str) -> AppResult<()> {
    WebviewWindowBuilder::new(app, label, WebviewUrl::App("index.html".into()))
        .title(title)
        .inner_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .min_inner_size(WINDOW_MIN_WIDTH, WINDOW_MIN_HEIGHT)
        .resizable(true)
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true)
        .theme(Some(tauri::Theme::Dark))
        .background_color(tauri::window::Color(0x08, 0x08, 0x0a, 0xff))
        .build()
        .map(|_| ())
        .map_err(|e| AppError::Internal(format!("创建新窗口失败: {e}")))
}
