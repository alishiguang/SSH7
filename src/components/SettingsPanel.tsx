import { useCallback, useEffect, useRef, useState } from 'react'

import { openExternal, REPO_URL } from '../api/bridge'
import {
  TERMINAL_FONT_DEFAULT,
  TERMINAL_FONT_MAX,
  TERMINAL_FONT_MIN,
  UI_SCALE_DEFAULT,
  UI_SCALE_MAX,
  UI_SCALE_MIN,
  useSettings,
} from '../store/settings'
import { useUi } from '../store/ui'

import { Icon } from './Icons'
import type { ConfirmSpec } from './ConfirmDialog'

interface Props {
  onClose: () => void
  /** 危险操作前的二次确认。 */
  askConfirm: (spec: ConfirmSpec) => void
}

/**
 * 设置面板。
 *
 * 只管**显示偏好**（字号、界面缩放）。
 * 凭据存放位置不再给用户选 —— 永远是本地加密文件。
 */
export default function SettingsPanel({ onClose, askConfirm }: Props) {
  const ref = useRef<HTMLDivElement>(null)

  const fontSize = useSettings((s) => s.terminalFontSize)
  const uiScale = useSettings((s) => s.uiScale)
  const nudgeTerminalFontSize = useSettings((s) => s.nudgeTerminalFontSize)
  const nudgeUiScale = useSettings((s) => s.nudgeUiScale)
  const reset = useSettings((s) => s.reset)

  const [version, setVersion] = useState<string | null>(null)

  /**
   * 打开仓库页面。
   *
   * 打不开就**说出来** —— 在 Tauri 的 webview 里，失败的链接是静默的，
   * 用户只会看到「点了没反应」。
   */
  const openRepo = useCallback(async () => {
    try {
      await openExternal(REPO_URL)
    } catch (e) {
      useUi.getState().push({
        kind: 'error',
        title: '打不开仓库页面',
        message: REPO_URL,
        hint: e instanceof Error ? e.message : String(e),
      })
    }
  }, [])

  // 当前版本号，显示在 GitHub 图标下面。
  useEffect(() => {
    let alive = true
    void (async () => {
      try {
        const { getVersion } = await import('@tauri-apps/api/app')
        const v = await getVersion()
        if (alive) setVersion(v)
      } catch {
        // 浏览器预览里没有 Tauri 应用 API，忽略即可。
      }
    })()
    return () => {
      alive = false
    }
  }, [])

  // 点击别处或按 Esc 关闭。
  useEffect(() => {
    const onPointerDown = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) onClose()
    }
    const onKey = (e: KeyboardEvent) => {
      // 确认框开着的时候，Esc 归它 —— 先关确认框，别把设置面板也关了。
      if (e.key === 'Escape' && !useUi.getState().confirmOpen) onClose()
    }
    window.addEventListener('mousedown', onPointerDown)
    // 捕获阶段监听：终端获得焦点时，xterm 会吞掉 keydown 的冒泡，
    // 用冒泡监听的话 Esc 就永远送不到这里。
    window.addEventListener('keydown', onKey, true)
    return () => {
      window.removeEventListener('mousedown', onPointerDown)
      window.removeEventListener('keydown', onKey, true)
    }
  }, [onClose])

  // 步进一律走 store 里的函数，内部读的是最新值，
  // 连点时不会因为渲染闭包里的旧值而只生效一次。
  const stepFont = (delta: number) => nudgeTerminalFontSize(delta)

  const stepScale = (delta: number) => nudgeUiScale(delta)

  return (
    <div className="settings-panel" ref={ref} role="dialog" aria-label="设置">
      <div className="settings-head">
        <span className="meta">// SETTINGS</span>
        <button className="icon-btn" title="关闭" onClick={onClose}>
          <Icon name="x" size={12} />
        </button>
      </div>

      <div className="settings-row">
        <span className="settings-label">终端字号</span>
        <div className="stepper">
          <button
            className="icon-btn"
            title="缩小字号"
            disabled={fontSize <= TERMINAL_FONT_MIN}
            onClick={() => stepFont(-1)}
          >
            <Icon name="minus" size={12} />
          </button>
          <span className="stepper-value">{fontSize.toFixed(1)} px</span>
          <button
            className="icon-btn"
            title="放大字号"
            disabled={fontSize >= TERMINAL_FONT_MAX}
            onClick={() => stepFont(1)}
          >
            <Icon name="plus" size={12} />
          </button>
        </div>
      </div>

      <div className="settings-row">
        <span className="settings-label">界面缩放</span>
        <div className="stepper">
          <button
            className="icon-btn"
            title="缩小界面"
            disabled={uiScale <= UI_SCALE_MIN}
            onClick={() => stepScale(-0.05)}
          >
            <Icon name="minus" size={12} />
          </button>
          <span className="stepper-value">{Math.round(uiScale * 100)} %</span>
          <button
            className="icon-btn"
            title="放大界面"
            disabled={uiScale >= UI_SCALE_MAX}
            onClick={() => stepScale(0.05)}
          >
            <Icon name="plus" size={12} />
          </button>
        </div>
      </div>

      <button
        className="btn btn-ghost btn-sm settings-reset"
        onClick={() =>
          askConfirm({
            title: 'RESET SETTINGS',
            message: (
              <>
                把界面设置恢复到出厂状态？
                <span className="confirm-hint">
                  终端字号与界面缩放会回到默认值。主机列表和凭据不受影响。
                </span>
              </>
            ),
            confirmLabel: '恢复默认',
            onConfirm: reset,
          })
        }
      >
        恢复默认
        <span className="settings-reset-hint">
          {TERMINAL_FONT_DEFAULT} px / {Math.round(UI_SCALE_DEFAULT * 100)}%
        </span>
      </button>

      <div className="settings-section">
        <span className="meta">// SHORTCUTS</span>
        <div className="settings-shortcut">
          <kbd>Ctrl</kbd>
          <kbd>+</kbd>
          <span>终端字号放大</span>
        </div>
        <div className="settings-shortcut">
          <kbd>Ctrl</kbd>
          <kbd>−</kbd>
          <span>终端字号缩小</span>
        </div>
        <div className="settings-shortcut">
          <kbd>Ctrl</kbd>
          <kbd>0</kbd>
          <span>终端字号复位</span>
        </div>
      </div>

      <div className="settings-foot">
        <button
          className="settings-github"
          onClick={() => void openRepo()}
          title={`完整功能说明见仓库：${REPO_URL}`}
          aria-label="打开 GitHub 仓库"
        >
          <Icon name="github" size={15} />
        </button>
        {version && <span className="settings-version">v{version}</span>}
      </div>
    </div>
  )
}
