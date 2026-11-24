import { useCallback, useEffect, useRef, useState } from 'react'

import { credentialApi, openExternal, REPO_URL } from '../api/bridge'
import type { CredentialBackend, CredentialStoreInfo } from '../api/types'
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

/** 两种后端的说明文案。 */
const BACKEND_NOTES: Record<CredentialBackend, string> = {
  vault: 'AES-256-GCM 加密，不弹窗',
  keyring: '系统托管，更安全',
}

/**
 * 设置面板。
 *
 * 显示偏好（字号、界面缩放）与凭据存储位置。
 * 这里只切换**存放位置**，任何时候都拿不到明文密码。
 */
export default function SettingsPanel({ onClose }: Props) {
  const ref = useRef<HTMLDivElement>(null)

  const fontSize = useSettings((s) => s.terminalFontSize)
  const uiScale = useSettings((s) => s.uiScale)
  const nudgeTerminalFontSize = useSettings((s) => s.nudgeTerminalFontSize)
  const nudgeUiScale = useSettings((s) => s.nudgeUiScale)
  const reset = useSettings((s) => s.reset)

  const [store, setStore] = useState<CredentialStoreInfo | null>(null)
  const [storeError, setStoreError] = useState<string | null>(null)
  const [switching, setSwitching] = useState(false)

  // 打开时读一次当前后端。
  useEffect(() => {
    let alive = true
    credentialApi
      .info()
      .then((info) => {
        if (alive) setStore(info)
      })
      .catch((e) => {
        if (alive) setStoreError(e instanceof Error ? e.message : String(e))
      })
    return () => {
      alive = false
    }
  }, [])

  const switchBackend = useCallback(async (backend: CredentialBackend) => {
    setSwitching(true)
    setStoreError(null)
    try {
      const info = await credentialApi.set(backend)
      setStore(info)
    } catch (e) {
      setStoreError(e instanceof Error ? e.message : String(e))
    } finally {
      setSwitching(false)
    }
  }, [])

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
        <span className="meta">// CREDENTIALS</span>
        <div className="settings-cred">
          {(['vault', 'keyring'] as CredentialBackend[]).map((b) => {
            const active = store?.backend === b
            const disabled = switching || (b === 'keyring' && store?.keyringAvailable === false)
            return (
              <button
                key={b}
                className={`settings-cred-option${active ? ' is-active' : ''}`}
                disabled={disabled}
                onClick={() => void switchBackend(b)}
              >
                <span className="settings-cred-name">
                  {b === 'vault' ? '本地加密文件' : '系统钥匙串'}
                </span>
                <span className="settings-cred-note">
                  {b === 'keyring' && store?.keyringAvailable === false
                    ? '本机不可用'
                    : BACKEND_NOTES[b]}
                </span>
              </button>
            )
          })}
        </div>
        <p className="settings-cred-tip">
          macOS 上未签名的应用每次升级都会被系统钥匙串要求重新授权，弹窗很烦；
          「本地加密文件」不会弹窗，密码以 AES-256-GCM 加密存在应用目录里。
        </p>
        {storeError && <p className="settings-cred-tip is-error">{storeError}</p>}
      </div>

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
        完整功能说明见仓库{' '}
        <button
          className="settings-foot-link"
          onClick={() => void openExternal(REPO_URL)}
          title={REPO_URL}
        >
          {REPO_URL}
        </button>
      </div>
    </div>
  )
}
