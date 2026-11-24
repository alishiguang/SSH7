import { useEffect, useRef, useState } from 'react'
import { Terminal as XTerm } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import { WebLinksAddon } from '@xterm/addon-web-links'
import '@xterm/xterm/css/xterm.css'

import { events, sessionApi, sftpApi } from '../api/bridge'
import { useDropZone } from '../hooks/useDropZone'
import { useTransfers } from '../store/transfers'

import { Icon } from './Icons'

interface Props {
  sessionId: string
  active: boolean
  /** 远端当前目录，用于拖拽上传的默认目标 */
  cwd?: string
}

/** 与设计令牌一致的 xterm 主题。 */
const THEME = {
  background: '#0b0b0e',
  foreground: '#f2efe6',
  cursor: '#f70121',
  cursorAccent: '#0b0b0e',
  selectionBackground: 'rgba(247, 1, 33, 0.28)',
  black: '#101014',
  red: '#f70121',
  green: '#3dd68c',
  yellow: '#e8b931',
  blue: '#5b8dee',
  magenta: '#c77dff',
  cyan: '#4fd6d2',
  white: '#f2efe6',
  brightBlack: '#5f5c56',
  brightRed: '#ff2440',
  brightGreen: '#6ee7a8',
  brightYellow: '#ffd75e',
  brightBlue: '#8bb0ff',
  brightMagenta: '#ddaaff',
  brightCyan: '#8ef0ec',
  brightWhite: '#ffffff',
}

export default function TerminalView({ sessionId, active, cwd }: Props) {
  const mountRef = useRef<HTMLDivElement>(null)
  const termRef = useRef<XTerm | null>(null)
  const fitRef = useRef<FitAddon | null>(null)
  const cwdRef = useRef<string | undefined>(cwd)

  const [uploading, setUploading] = useState(false)
  const { ref: zoneRef, isOver, paths } = useDropZone<HTMLDivElement>()

  const transfers = useTransfers((s) => s.transfers)
  const startTransfer = useTransfers((s) => s.start)
  const finishTransfer = useTransfers((s) => s.finish)

  cwdRef.current = cwd

  /* ── 终端生命周期 ───────────────────────────── */
  useEffect(() => {
    const mount = mountRef.current
    if (!mount) return

    const term = new XTerm({
      theme: THEME,
      fontFamily:
        '"Geist Mono", ui-monospace, "SF Mono", "JetBrains Mono", Menlo, monospace',
      fontSize: 12.5,
      lineHeight: 1.45,
      letterSpacing: 0.2,
      cursorBlink: true,
      cursorStyle: 'bar',
      scrollback: 8000,
      smoothScrollDuration: 90,
      allowProposedApi: true,
    })

    const fit = new FitAddon()
    term.loadAddon(fit)
    term.loadAddon(new WebLinksAddon())
    term.open(mount)

    termRef.current = term
    fitRef.current = fit

    // 初次 fit：等容器拿到真实尺寸
    const raf = requestAnimationFrame(() => {
      try {
        fit.fit()
        void sessionApi.resize(sessionId, term.cols, term.rows)
      } catch {
        /* 会话可能已关闭 */
      }
    })

    // 键盘输入 → SSH
    const dataSub = term.onData((data) => {
      void sessionApi.write(sessionId, data).catch(() => {})
    })

    // 尺寸变化
    const ro = new ResizeObserver(() => {
      try {
        fit.fit()
        void sessionApi.resize(sessionId, term.cols, term.rows)
      } catch {
        /* ignore */
      }
    })
    ro.observe(mount)

    // 事件监听必须先注册，再让 Rust 补发缓存的首批输出，否则会丢首屏。
    let disposed = false
    let unlistenData: (() => void) | undefined
    let unlistenClosed: (() => void) | undefined

    void (async () => {
      unlistenData = await events.onData((e) => {
        if (e.sessionId === sessionId) term.write(e.data)
      })
      unlistenClosed = await events.onClosed((e) => {
        if (e.sessionId !== sessionId) return
        term.write('\r\n\x1b[38;2;247;1;33m[连接已断开]\x1b[0m\r\n')
      })
      if (disposed) {
        unlistenData?.()
        unlistenClosed?.()
        return
      }
      await sessionApi.attach(sessionId).catch(() => {})
    })()

    return () => {
      disposed = true
      cancelAnimationFrame(raf)
      ro.disconnect()
      dataSub.dispose()
      // 组件卸载时必须解除监听，否则会像旧版一样无限累积（缺陷 #7）
      unlistenData?.()
      unlistenClosed?.()
      term.dispose()
      termRef.current = null
      fitRef.current = null
    }
  }, [sessionId])

  /* ── 标签激活时重新 fit + 聚焦 ─────────────── */
  useEffect(() => {
    if (!active) return
    const t = window.setTimeout(() => {
      try {
        fitRef.current?.fit()
        if (termRef.current) {
          void sessionApi.resize(sessionId, termRef.current.cols, termRef.current.rows)
          termRef.current.focus()
        }
      } catch {
        /* ignore */
      }
    }, 50)
    return () => window.clearTimeout(t)
  }, [active, sessionId])

  /* ── 拖拽上传 ───────────────────────────────── */
  const uploadPaths = paths.length > 0 ? paths : []
  const consumedRef = useRef<string>('')

  useEffect(() => {
    if (!isOver || uploadPaths.length === 0) return
    // 同一个拖放动作只消费一次
    const key = uploadPaths.join('|')
    if (consumedRef.current === key) return
    consumedRef.current = key
    void (async () => {
      setUploading(true)
      const target = cwdRef.current && cwdRef.current !== '~' ? cwdRef.current : '.'
      for (const localPath of uploadPaths) {
        const name = localPath.split(/[\\/]/).pop() || localPath
        const transferId = crypto.randomUUID()
        startTransfer({
          id: transferId,
          sessionId,
          fileName: name,
          transferred: 0,
          total: 0,
          direction: 'upload',
        })
        try {
          await sftpApi.upload(sessionId, localPath, target, transferId)
          finishTransfer(transferId)
          termRef.current?.write(
            `\x1b[38;2;61;214;140m✓ 上传完成\x1b[0m ${name} → ${target}\r\n`,
          )
        } catch (e) {
          const msg = e instanceof Error ? e.message : String(e)
          finishTransfer(transferId, msg)
          termRef.current?.write(
            `\x1b[38;2;247;1;33m✗ 上传失败\x1b[0m ${name}: ${msg}\r\n`,
          )
        }
      }
      setUploading(false)
      consumedRef.current = ''
    })()
  }, [isOver, uploadPaths, sessionId, startTransfer, finishTransfer])

  const sessionTransfers = transfers.filter((t) => t.sessionId === sessionId)

  return (
    <div className="terminal-wrap" ref={(el) => { zoneRef.current = el }}>
      <div className="terminal-mount" ref={mountRef} />

      {isOver && (
        <div className="drop-overlay">
          <div className="drop-overlay-inner">
            <Icon name="upload" size={26} />
            <div className="drop-overlay-title">松开即可上传</div>
            <div className="drop-overlay-sub">
              目标目录：<code>{cwd ?? '远端默认目录'}</code>
            </div>
          </div>
        </div>
      )}

      {uploading && !isOver && (
        <div className="terminal-uploading">
          <span className="spinner" /> 正在上传…
        </div>
      )}

      {sessionTransfers.length > 0 && (
        <div className="transfer-stack">
          <div className="meta transfer-stack-head">TRANSFER</div>
          {sessionTransfers.map((t) => (
            <div className="transfer-item" key={t.id}>
              <div className="transfer-row">
                <Icon
                  name={t.direction === 'upload' ? 'upload' : 'download'}
                  size={11}
                  className="transfer-icon"
                />
                <span className="transfer-name">{t.fileName}</span>
                <span
                  className={`transfer-pct${
                    t.status === 'error' ? ' is-error' : t.status === 'done' ? ' is-done' : ''
                  }`}
                >
                  {t.status === 'error' ? '失败' : t.status === 'done' ? '完成' : `${t.percent}%`}
                </span>
              </div>
              <div className="transfer-bar">
                <div
                  className={`transfer-bar-fill${
                    t.status === 'error' ? ' is-error' : t.status === 'done' ? ' is-done' : ''
                  }`}
                  style={{ width: `${t.percent}%` }}
                />
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  )
}
