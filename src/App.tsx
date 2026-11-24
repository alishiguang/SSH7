import { useCallback, useEffect, useMemo, useRef, useState } from 'react'

import { events, sessionApi } from './api/bridge'
import type { ConnectRequest, SavedHost } from './api/types'
import ConnectDialog from './components/ConnectDialog'
import ContextMenu, { type ContextMenuItem } from './components/ContextMenu'
import EmptyState from './components/EmptyState'
import FilePanel from './components/FilePanel'
import { Icon } from './components/Icons'
import SessionTabs from './components/SessionTabs'
import SettingsPanel from './components/SettingsPanel'
import Sidebar from './components/Sidebar'
import TerminalView from './components/TerminalView'
import Toasts from './components/Toasts'
import { useDrop } from './store/drop'
import { useHosts } from './store/hosts'
import { useSessions } from './store/sessions'
import {
  TERMINAL_FONT_DEFAULT,
  applyUiScale,
  useSettings,
} from './store/settings'
import { useTransfers } from './store/transfers'
import { useUi } from './store/ui'

/** 占位会话的序号：连接建立之前先用它占个位置。 */
let pendingSeq = 0

export default function App() {
  const sessions = useSessions((s) => s.sessions)
  const activeId = useSessions((s) => s.activeId)
  const beginSession = useSessions((s) => s.begin)
  const patchSession = useSessions((s) => s.patch)
  const replaceSession = useSessions((s) => s.replace)
  const removeSession = useSessions((s) => s.remove)
  const setActive = useSessions((s) => s.setActive)

  const hosts = useHosts((s) => s.hosts)
  const loadHosts = useHosts((s) => s.load)

  const filePanelOpen = useUi((s) => s.filePanelOpen)
  const setFilePanel = useUi((s) => s.setFilePanel)
  const reportError = useUi((s) => s.reportError)
  const push = useUi((s) => s.push)
  /* 正在改名的会话（标签栏双击或右键菜单触发）。
     放在 store 里，终端才知道「这会儿别抢焦点」。 */
  const renamingSession = useUi((s) => s.renamingSessionId)
  const setRenamingSession = useUi((s) => s.setRenamingSession)
  const transfers = useTransfers((s) => s)

  const [dialogOpen, setDialogOpen] = useState(false)
  const [editingHost, setEditingHost] = useState<SavedHost | null>(null)
  const [settingsOpen, setSettingsOpen] = useState(false)

  const initDrop = useDrop((s) => s.init)

  /* ── 启动：加载主机、注册原生拖放监听 ────────── */
  useEffect(() => {
    void loadHosts().catch((e) => reportError('读取主机列表失败', e as never))
    void initDrop()
  }, [loadHosts, initDrop, reportError])

  /* ── 界面缩放：写入根元素，供 CSS 令牌计算字号 ── */
  const uiScale = useSettings((s) => s.uiScale)
  useEffect(() => {
    applyUiScale(uiScale)
  }, [uiScale])

  /* ── 快捷键：Ctrl +/-/0 调整终端字号 ────────── */
  const nudgeTerminalFontSize = useSettings((s) => s.nudgeTerminalFontSize)
  const setTerminalFontSize = useSettings((s) => s.setTerminalFontSize)
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      // 只用 Ctrl，避免和 macOS 的 Cmd+ 缩放打架。
      if (!e.ctrlKey || e.metaKey || e.altKey) return
      const t = e.target as HTMLElement | null
      if (
        t &&
        (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.isContentEditable)
      ) {
        return
      }
      if (e.key === '+' || e.key === '=') {
        e.preventDefault()
        nudgeTerminalFontSize(1)
      } else if (e.key === '-' || e.key === '_') {
        e.preventDefault()
        nudgeTerminalFontSize(-1)
      } else if (e.key === '0') {
        e.preventDefault()
        setTerminalFontSize(TERMINAL_FONT_DEFAULT)
      }
    }
    window.addEventListener('keydown', onKey, true)
    return () => window.removeEventListener('keydown', onKey, true)
  }, [nudgeTerminalFontSize, setTerminalFontSize])

  /* ── 全局事件监听（只注册一次）──────────────── */
  useEffect(() => {
    const cleanups: Array<() => void> = []

    void events
      .onCwd((e) => {
        patchSession(e.sessionId, { cwd: e.cwd })
      })
      .then((un) => cleanups.push(un))

    void events
      .onClosed((e) => {
        patchSession(e.sessionId, { status: 'disconnected' })
      })
      .then((un) => cleanups.push(un))

    void events
      .onError((e) => {
        patchSession(e.sessionId, { status: 'failed', error: e.error })
        reportError('会话出错', e.error)
      })
      .then((un) => cleanups.push(un))

    // 传输进度按 transfer_id 关联（缺陷 #6），不再用文件名匹配。
    void events
      .onTransfer((e) => {
        transfers.progress(e.transferId, e.transferred, e.total, e.percent)
      })
      .then((un) => cleanups.push(un))

    return () => {
      for (const c of cleanups) c()
    }
  }, [patchSession, reportError, transfers])

  /* ── 会话被移除时真正断开连接 ───────────────────
     缺陷 #2 / #9：旧版关闭标签只是把 UI 移除，SSH 连接一直挂着。
     这里统一在「会话从列表消失」时调用 disconnect，无论关闭动作
     由哪个入口触发都不会漏。                                  */
  const knownIds = useRef<Set<string>>(new Set())
  useEffect(() => {
    const current = new Set(sessions.map((s) => s.id))
    for (const id of knownIds.current) {
      if (!current.has(id)) {
        void sessionApi.disconnect(id).catch(() => {})
      }
    }
    knownIds.current = current
  }, [sessions])

  /* ── 窗口关闭前断开全部连接 ─────────────────── */
  useEffect(() => {
    let dispose: (() => void) | undefined
    void (async () => {
      try {
        const { getCurrentWindow } = await import('@tauri-apps/api/window')
        dispose = await getCurrentWindow().onCloseRequested(() => {
          for (const s of useSessions.getState().sessions) {
            void sessionApi.disconnect(s.id).catch(() => {})
          }
        })
      } catch {
        // 非 Tauri 环境（如浏览器预览）拿不到窗口 API，忽略即可。
      }
    })()
    return () => dispose?.()
  }, [])

  /* ── 建立连接 ───────────────────────────────── */
  const handleConnect = useCallback(
    async (req: ConnectRequest, opts?: { pending?: boolean }) => {
      // 从主机列表点进来时，先把「正在连接」摆出来：完整走一遍 SSH 握手
      // 要小半秒，没有占位会话的话界面看起来就是没反应（缺陷 #22）。
      const pendingId = opts?.pending ? `pending-${++pendingSeq}` : null
      if (pendingId) {
        beginSession({
          id: pendingId,
          label: req.label || `${req.username}@${req.host}`,
          host: req.host,
          port: req.port ?? 22,
          username: req.username,
          authType: req.authType,
        })
      }
      try {
        const info = await sessionApi.connect(req)
        if (pendingId) {
          // 换成后端的真实会话：标签页原地替换，不闪。
          replaceSession(pendingId, {
            id: info.sessionId,
            label: info.label,
            host: info.host,
            port: info.port,
            username: info.username,
            authType: info.authType,
            status: 'connected',
          })
        } else {
          beginSession({
            id: info.sessionId,
            label: info.label,
            host: info.host,
            port: info.port,
            username: info.username,
            authType: info.authType,
          })
          patchSession(info.sessionId, { status: 'connected' })
        }
        setDialogOpen(false)
        setEditingHost(null)
        push({ kind: 'success', title: '已连接', message: info.label })
        // 保存主机后刷新左侧列表（Rust 侧已经落盘）。
        if (req.save) {
          void loadHosts().catch(() => {})
        }
      } catch (e) {
        // 连不上就把占位会话收掉，错误照旧用提示条报给用户。
        if (pendingId) removeSession(pendingId)
        reportError('连接失败', e as never)
        throw e
      }
    },
    [beginSession, patchSession, replaceSession, removeSession, push, reportError, loadHosts],
  )

  /** 强制为某台已保存主机新开一个会话。 */
  const openHostSession = useCallback(
    async (host: SavedHost) => {
      // 复用的主机直接连接；秘密由 Rust 从本地加密库读取，不经过前端。
      try {
        await handleConnect(
          {
            host: host.host,
            port: host.port,
            username: host.username,
            authType: host.authType,
            privateKeyPath: host.privateKeyPath,
            save: false,
            label: host.label,
            hostId: host.id,
          },
          { pending: true },
        )
      } catch {
        // 错误已经由 handleConnect 报过了。
      }
    },
    [handleConnect],
  )

  const handleSelectHost = useCallback(
    (host: SavedHost) => {
      // 已经存在（或正在建立）同一目标主机会话时，直接切过去而不是再开一个。
      const existing = useSessions
        .getState()
        .findByTarget(host.host, host.port, host.username)
      if (existing) {
        setActive(existing.id)
        return
      }
      void openHostSession(host)
    },
    [openHostSession, setActive],
  )

  /** 克隆会话：凭据在 Rust 侧复用，前端不接触明文密码。 */
  const handleCloneSession = useCallback(
    async (sessionId: string) => {
      try {
        // 沿用用户在标签栏里改过的名字
        const label = useSessions.getState().sessions.find((s) => s.id === sessionId)?.label
        const info = await sessionApi.clone(sessionId, label)
        beginSession({
          id: info.sessionId,
          label: info.label,
          host: info.host,
          port: info.port,
          username: info.username,
          authType: info.authType,
        })
        patchSession(info.sessionId, { status: 'connected' })
        setActive(info.sessionId)
        push({ kind: 'success', title: '已克隆会话', message: info.label })
      } catch (e) {
        reportError('克隆会话失败', e as never)
      }
    },
    [beginSession, patchSession, setActive, push, reportError],
  )

  /* ── 会话右键菜单 ─────────────────────────────── */
  const [menu, setMenu] = useState<{
    x: number
    y: number
    sessionId: string
  } | null>(null)

  const menuItems = useMemo<ContextMenuItem[]>(() => {
    if (!menu) return []
    const s = sessions.find((x) => x.id === menu.sessionId)
    if (!s) return []
    return [
      {
        id: 'rename',
        label: '重命名',
        icon: 'edit',
        onSelect: () => setRenamingSession(s.id),
      },
      {
        id: 'clone',
        label: '克隆为新会话',
        icon: 'plus',
        onSelect: () => void handleCloneSession(s.id),
      },
      {
        id: 'disconnect',
        label: '断开连接',
        icon: 'refresh',
        onSelect: () => {
          void sessionApi.disconnect(s.id).catch(() => {})
          patchSession(s.id, { status: 'disconnected' })
        },
      },
      {
        id: 'close',
        label: '关闭会话',
        icon: 'x',
        danger: true,
        onSelect: () => removeSession(s.id),
      },
    ]
  }, [menu, sessions, handleCloneSession, patchSession, removeSession])

  const activeSession = sessions.find((s) => s.id === activeId)

  return (
    <div className="app">
      <div className="titlebar" data-tauri-drag-region>
        <span className="titlebar-brand display" data-tauri-drag-region>
          SSH7
        </span>
        <button
          className="titlebar-btn"
          title="设置"
          aria-label="设置"
          onClick={() => setSettingsOpen((v) => !v)}
        >
          <Icon name="settings" size={13} />
        </button>
      </div>

      <div className="app-body">
        <Sidebar
          onConnect={() => {
            setEditingHost(null)
            setDialogOpen(true)
          }}
          onSelectHost={handleSelectHost}
          onOpenNewSession={(host) => void openHostSession(host)}
          onEditHost={(h) => {
            setEditingHost(h)
            setDialogOpen(true)
          }}
        />

        <main className="workspace">
          <SessionTabs
            onSessionMenu={(sessionId, x, y) => setMenu({ sessionId, x, y })}
            onRenameStart={(sessionId) => setRenamingSession(sessionId)}
            renamingId={renamingSession}
            onRenameEnd={(sessionId, label) => {
              setRenamingSession(null)
              if (!label) return
              patchSession(sessionId, { label })
            }}
          />

          <div className="stage">
            {sessions.length === 0 ? (
              <EmptyState
                hostCount={hosts.length}
                onConnect={() => {
                  setEditingHost(null)
                  setDialogOpen(true)
                }}
              />
            ) : (
              sessions.map((s) => (
                <div
                  key={s.id}
                  className={`stage-pane${s.id === activeId ? ' is-active' : ''}`}
                  style={{ visibility: s.id === activeId ? 'visible' : 'hidden' }}
                >
                  {s.status === 'connected' ? (
                    <TerminalView sessionId={s.id} active={s.id === activeId} cwd={s.cwd} />
                  ) : s.status === 'connecting' ? (
                    <div className="stage-status">
                      <span className="spinner" />
                      <span>正在连接 {s.label}…</span>
                    </div>
                  ) : (
                    <div className="stage-status is-error">
                      <div className="stage-error">
                        <div className="stage-error-title">
                          {s.status === 'failed' ? '连接失败' : '连接已断开'}
                        </div>
                        {s.error && (
                          <>
                            <div className="stage-error-msg">{s.error.message}</div>
                            {s.error.hint && <div className="stage-error-hint">{s.error.hint}</div>}
                          </>
                        )}
                        <button
                          className="btn btn-ghost btn-sm"
                          onClick={() => {
                            removeSession(s.id)
                            void handleConnect({
                              label: s.label,
                              host: s.host,
                              port: s.port,
                              username: s.username,
                              authType: s.authType,
                              save: false,
                            })
                          }}
                        >
                          重试
                        </button>
                      </div>
                    </div>
                  )}
                </div>
              ))
            )}
          </div>
        </main>

        {filePanelOpen && activeSession && activeSession.status === 'connected' && (
          <FilePanel
            key={activeSession.id}
            sessionId={activeSession.id}
            onClose={() => setFilePanel(false)}
          />
        )}
      </div>

      {dialogOpen && (
        <ConnectDialog
          editHost={editingHost}
          onSubmit={handleConnect}
          onClose={() => {
            setDialogOpen(false)
            setEditingHost(null)
          }}
        />
      )}

      {menu && menuItems.length > 0 && (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          items={menuItems}
          onClose={() => setMenu(null)}
        />
      )}

      {settingsOpen && (
        <SettingsPanel onClose={() => setSettingsOpen(false)} />
      )}

      <Toasts />
    </div>
  )
}
