import { useEffect, useRef } from 'react'

import {
  TERMINAL_FONT_DEFAULT,
  TERMINAL_FONT_MAX,
  TERMINAL_FONT_MIN,
  UI_SCALE_DEFAULT,
  UI_SCALE_MAX,
  UI_SCALE_MIN,
  useSettings,
} from '../store/settings'

import { Icon } from './Icons'

interface Props {
  onClose: () => void
}

/**
 * 设置面板。
 *
 * 只放**显示偏好**（字号、界面缩放），不涉及任何凭据。
 */
export default function SettingsPanel({ onClose }: Props) {
  const ref = useRef<HTMLDivElement>(null)

  const fontSize = useSettings((s) => s.terminalFontSize)
  const uiScale = useSettings((s) => s.uiScale)
  const nudgeTerminalFontSize = useSettings((s) => s.nudgeTerminalFontSize)
  const nudgeUiScale = useSettings((s) => s.nudgeUiScale)
  const reset = useSettings((s) => s.reset)

  // 点击别处或按 Esc 关闭。
  useEffect(() => {
    const onPointerDown = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) onClose()
    }
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose()
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

      <button className="btn btn-ghost btn-sm settings-reset" onClick={reset}>
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

      <div className="settings-foot">完整功能说明见仓库 docs/使用手册.md</div>
    </div>
  )
}
