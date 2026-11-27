import { useCallback, useEffect, useMemo, useRef, useState } from 'react'

import type { UnlistenFn } from '@tauri-apps/api/event'

import { events, sessionApi, windowApi } from './api/bridge'
import type {
  AppErrorPayload,
  ConnectRequest,
  HostSaveRequest,
  SavedHost,
} from './api/types'
import ConnectDialog from './components/ConnectDialog'
import ConfirmDialog, { type ConfirmSpec } from './components/ConfirmDialog'
import ContextMenu, { type ContextMenuItem } from './components/ContextMenu'
import EmptyState from './components/EmptyState'
import FilePanel from './components/FilePanel'
import { Icon } from './components/Icons'
import SessionOverlay from './components/SessionOverlay'
import SessionTabs from './components/SessionTabs'
import SettingsPanel from './components/SettingsPanel'
import Sidebar from './components/Sidebar'
import TerminalView from './components/TerminalView'
import Toasts from './components/Toasts'
import { useDrop } from './store/drop'
import { useHosts } from './store/hosts'
import {
  buildReconnectRequest,
  RECONNECT_DELAYS,
  useSessions,
} from './store/sessions'
import {
  TERMINAL_FONT_DEFAULT,
  applyUiScale,
  useSettings,
} from './store/settings'
import { useTransfers } from './store/transfers'
import { useUi } from './store/ui'

/** 占位会话的序号：连接建立之前先用它占个位置。 */
let pendingSeq = 0

/** 等一会儿（自动重连的退避）。 */
function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

/**
 * 把后端抛出来的东西统一成结构化错误。
 *
 * 自动重连彻底失败时，界面上要能说清「为什么没连上」——
 * 总不能只丢一句「连接已断开」让人干瞪眼。
 */
function toAppError(e: unknown): AppErrorPayload {
  if (e && typeof e === 'object' && 'code' in e) return e as AppErrorPayload
  const err = e as Error | undefined
  return {
    code: 'INTERNAL',
    message: err?.message ? err.message : String(e),
    detail: 'detail' in (e as object) ? String((e as { detail?: unknown }).detail) : undefined,
  }
}

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
  const saveHost = useHosts((s) => s.save)
  const removeHost = useHosts((s) => s.remove)

  const filePanelOpen = useUi((s) => s.filePanelOpen)
  const setFilePanel = useUi((s) => s.setFilePanel)
  const reportError = useUi((s) => s.reportError)
  const push = useUi((s) => s.push)
  /* 正在改名的会话（标签栏双击或右键菜单触发）。
     放在 store 里，终端才知道「这会儿别抢焦点」。 */
  const renamingSession = useUi((s) => s.renamingSessionId)
  const setRenamingSession = useUi((s) => s.setRenamingSession)
  /** 只取动作本身 —— store 的 state 每次更新都是新对象，拿它当依赖会让监听反复重注册。 */
  const transferProgress = useTransfers((s) => s.progress)

  const [dialogOpen, setDialogOpen] = useState(false)
  const [editingHost, setEditingHost] = useState<SavedHost | null>(null)
  /**
   * 新建连接时指定落地的分组（从分组右键进来的）。
   * null = 不指定 —— 按「当前选中的主机所在的分组」算（见 D19）。
   */
  const [newConnGroup, setNewConnGroup] = useState<string | null>(null)
  const [settingsOpen, setSettingsOpen] = useState(false)
  /**
   * 待确认的删除操作。null 表示没有。
   * 凡是会丢东西的操作都先走这里 —— 一次误点不该换来配置消失。
   */
  const [confirm, setConfirm] = useState<ConfirmSpec | null>(null)
  const askConfirm = useCallback((spec: ConfirmSpec) => setConfirm(spec), [])

  const initDrop = useDrop((s) => s.init)

  /**
   * 用户**主动**断开的会话。
   *
   * 这些断开之后不该自动重连 —— 用户点「断开连接」就是想断着，
   * 结果它自己又连回来，比不重连还烦。
   */
  const userClosedRef = useRef<Set<string>>(new Set())

  /**
   * 正在重连的会话。
   *
   * 同一条断开事件有可能被送到好几次（监听器重复注册、远端连着报了几次
   * closed），每来一次就开一条重连循环的话，会同时发出好几个连接请求 ——
   * 轻则刷屏，重则把服务器连接数打满。
   *
   * 这里给每个会话上一把锁：已经在重连的直接跳过。
   */
  const reconnectingRef = useRef<Set<string>>(new Set())

  /**
   * 自动重连的回调转发。
   *
   * 事件监听那个 effect 只注册一次，不能因为回调换了个引用就重跑 ——
   * 用 ref 转发出去，effect 的依赖就干净了。
   */
  const autoReconnectRef = useRef<(sessionId: string) => Promise<void>>(
    async () => {},
  )

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
    // 监听注册是**异步**的，而 effect 可能在这期间就被清理掉
    // （StrictMode 的双挂载、依赖变化）。那会儿拿到的 unlisten 必须立刻注销 ——
    // 否则监听器会越攒越多：一次断线就会触发好几次重连。
    let disposed = false
    const cleanups: Array<() => void> = []
    const reg = (p: Promise<UnlistenFn>) => {
      void p.then((un) => {
        if (disposed) un()
        else cleanups.push(un)
      })
    }

    reg(
      events.onCwd((e) => {
        patchSession(e.sessionId, { cwd: e.cwd })
      }),
    )

    reg(
      events.onClosed((e) => {
        // 标签页已经关了（会话被移除）—— 不用管。
        const s = useSessions.getState().sessions.find((x) => x.id === e.sessionId)
        if (!s) return
        patchSession(e.sessionId, { status: 'disconnected' })
        // 用户自己点的「断开连接」—— 就该断着，别自己接回去。
        if (userClosedRef.current.has(e.sessionId)) {
          userClosedRef.current.delete(e.sessionId)
          return
        }
        // 意外断的（网络抖动、远端重启、会话超时）：自己接回去。
        void autoReconnectRef.current(e.sessionId)
      }),
    )

    reg(
      events.onError((e) => {
        patchSession(e.sessionId, { status: 'failed', error: e.error })
        reportError('会话出错', e.error)
      }),
    )

    // 传输进度按 transfer_id 关联（缺陷 #6），不再用文件名匹配。
    reg(
      events.onTransfer((e) => {
        transferProgress(e.transferId, e.transferred, e.total, e.percent)
      }),
    )

    // 「定位到那扇窗」—— 别的窗口里开着同一台机器时，它会叫我们切过去。
    reg(
      events.onFocusSession((e) => {
        setActive(e.sessionId)
      }),
    )

    return () => {
      disposed = true
      for (const c of cleanups) c()
    }
  }, [patchSession, reportError, transferProgress, setActive])

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

  /* ── 窗口关闭：不劫持原生关闭 ─────────────────── */
  //
  // 这里**故意不注册** `onCloseRequested`。
  //
  // Tauri 的规矩是：webview 一旦监听了「关闭请求」，它就会先取消原生关闭
  // （`prevent_close`），改由前端接手 —— 前端必须自己再调一次 `destroy()`
  // 才算关掉。而 `destroy()` 是要过权限的：`core:window:default` 里**没有**
  // `allow-destroy`。所以一旦注册了这个监听，又没配相应权限，窗口就永远
  // 关不掉（点左上角红点没反应）。
  //
  // 关窗要做的清理（断开这扇窗的所有 SSH 会话）放在 Rust 侧：
  // 窗口 `Destroyed` 时统一回收 —— 不依赖前端还在不在（前端马上就死了）。

  /* ── 建立连接 ───────────────────────────────── */
  /**
   * 真正去连一次，成功返回可以直接写进 store 的会话状态。
   *
   * **凭据来源必须记下来**（`hostId` / `privateKeyPath`）——
   * 断线之后要靠它们把密码从加密凭据库里取回来；没有它们，就再也连不上。
   */
  const doConnect = useCallback(async (req: ConnectRequest) => {
    const info = await sessionApi.connect(req)
    return {
      id: info.sessionId,
      label: info.label,
      host: info.host,
      port: info.port,
      username: info.username,
      authType: info.authType,
      hostId: info.hostId,
      privateKeyPath: req.privateKeyPath,
    }
  }, [])

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
        const next = await doConnect(req)
        if (pendingId) {
          // 换成后端的真实会话：标签页原地替换，不闪。
          replaceSession(pendingId, { ...next, status: 'connected' })
        } else {
          beginSession(next)
          patchSession(next.id, { status: 'connected' })
        }
        setDialogOpen(false)
        setEditingHost(null)
        push({ kind: 'success', title: '已连接', message: next.label })
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
    [
      beginSession,
      patchSession,
      replaceSession,
      removeSession,
      push,
      reportError,
      loadHosts,
      doConnect,
    ],
  )

  /* ── 重新连接 ─────────────────────────────────── */
  /**
   * 手动重连：用户点「重新连接」按钮时走这里。
   *
   * **原地重连** —— 标签页不新建、不挪位置，成功后把会话换成新的
   * （后端每次连接都会发新的 session id）。
   */
  const handleReconnect = useCallback(
    async (sessionId: string) => {
      const current = useSessions.getState().sessions.find((s) => s.id === sessionId)
      if (!current) return
      // 已经在重连了就别再发一次 —— 连点两下「重新连接」不该开两条连接。
      if (reconnectingRef.current.has(sessionId)) return
      reconnectingRef.current.add(sessionId)
      patchSession(sessionId, { status: 'reconnecting', error: undefined, attempt: 1 })
      try {
        const next = await doConnect(buildReconnectRequest(current))
        replaceSession(sessionId, { ...next, status: 'connected' })
        push({ kind: 'success', title: '已重新连接', message: next.label })
      } catch (e) {
        patchSession(sessionId, { status: 'disconnected', error: toAppError(e) })
        reportError('重新连接失败', e as never)
      } finally {
        reconnectingRef.current.delete(sessionId)
      }
    },
    [doConnect, patchSession, replaceSession, push, reportError],
  )

  /**
   * 自动重连：连接**意外**断掉时自己接回去。
   *
   * 用户主动点的「断开连接」不算 —— 那种断开就该老老实实断着，
   * 否则用户会发现「我明明断开了，它又连回来了」。
   */
  const autoReconnect = useCallback(
    async (sessionId: string) => {
      // 同一条断开事件可能被送到好几次 —— 只允许一条重连循环。
      if (reconnectingRef.current.has(sessionId)) return
      reconnectingRef.current.add(sessionId)
      // 最后一次失败的原因 —— 全都试完了就把它摆到界面上。
      let lastError: AppErrorPayload | undefined
      try {
        for (let i = 0; i < RECONNECT_DELAYS.length; i++) {
          const before = useSessions.getState().sessions.find((s) => s.id === sessionId)
          if (!before) return
          patchSession(sessionId, {
            status: 'reconnecting',
            error: undefined,
            attempt: i + 1,
          })
          if (RECONNECT_DELAYS[i] > 0) await sleep(RECONNECT_DELAYS[i])
          // 等退避的这段时间里，用户可能已经把标签关了。
          const current = useSessions.getState().sessions.find((s) => s.id === sessionId)
          if (!current) return
          try {
            const next = await doConnect(buildReconnectRequest(current))
            replaceSession(sessionId, { ...next, status: 'connected' })
            push({ kind: 'success', title: '已重新连接', message: next.label })
            return
          } catch (e) {
            // 这一次没成，退避后再试。
            lastError = toAppError(e)
          }
        }
        // 都试过了还是不行 —— 停下来，让用户自己决定要不要继续。
        patchSession(sessionId, {
          status: 'disconnected',
          attempt: RECONNECT_DELAYS.length,
          error: lastError,
        })
      } finally {
        reconnectingRef.current.delete(sessionId)
      }
    },
    [doConnect, patchSession, replaceSession, push],
  )

  // 转发给事件监听用（见 autoReconnectRef 的说明）。
  autoReconnectRef.current = autoReconnect

  /** 「仅保存」：只写进主机列表，不建立连接。 */
  const handleSaveOnly = useCallback(
    async (payload: HostSaveRequest) => {
      // 秘密交给凭据库，hosts.json 里只留标记 —— 前端不落盘明文。
      await saveHost(payload.host, {
        password: payload.password,
        passphrase: payload.passphrase,
      })
      setDialogOpen(false)
      setEditingHost(null)
      push({ kind: 'success', title: '已保存', message: payload.host.label })
    },
    [saveHost, push],
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

  /** 切到该主机已有的会话（没有就不切）。 */
  const switchToHostSession = useCallback(
    (host: SavedHost) => {
      const existing = useSessions
        .getState()
        .findByTarget(host.host, host.port, host.username)
      if (existing) setActive(existing.id)
    },
    [setActive],
  )

  /**
   * 双击主机：在**当前窗口**里激活这台机器。
   *
   * 已经开着就切到那个标签页（双击不该开出两个重复的会话），
   * 没开着才新连一个。
   */
  const handleActivateHost = useCallback(
    async (host: SavedHost) => {
      const existing = useSessions
        .getState()
        .findByTarget(host.host, host.port, host.username)
      if (existing) {
        setActive(existing.id)
        return
      }
      await openHostSession(host)
    },
    [openHostSession, setActive],
  )

  /**
   * 把这台机器开到一个**独立窗口**里。
   *
   * 从主机右键菜单触发。已经开着就别再开一扇 —— Rust 侧知道那扇窗在哪，
   * 直接把它叫到前面；开着好几扇就挑一扇（「切换到其中一个」）。
   */
  const handleOpenInWindow = useCallback(
    async (host: SavedHost) => {
      try {
        const result = await windowApi.openInWindow({
          hostId: host.id,
          host: host.host,
          port: host.port,
          username: host.username,
          label: host.label,
        })
        if (result.action === 'switch' && result.sessionId) {
          // 就在当前窗口里 —— 切标签就行。
          setActive(result.sessionId)
        } else if (result.action === 'focused') {
          push({
            kind: 'info',
            title: '已定位',
            message: `${host.label} 已经在另一个窗口开着`,
          })
        } else {
          push({ kind: 'success', title: '已在新窗口打开', message: host.label })
        }
      } catch (e) {
        reportError('打开新窗口失败', e as never)
      }
    },
    [setActive, push, reportError],
  )

  /* ── 新窗口启动：自动连上被指派的那台机器 ───────────
     双击主机开出来的新窗口靠这个连上目标 —— 参数从 Rust 侧拿，
     不经过 URL，也就没有「参数还没到就先连上了」的竞态。          */
  useEffect(() => {
    let cancelled = false
    void (async () => {
      let assignment
      try {
        assignment = await windowApi.boot()
      } catch {
        return
      }
      if (!assignment || cancelled) return
      // 主机列表可能还没加载好，先确保拿到。
      await loadHosts().catch(() => {})
      if (cancelled) return
      const pool = useHosts.getState().hosts
      const target = assignment.hostId
        ? pool.find((h) => h.id === assignment.hostId)
        : pool.find(
            (h) =>
              h.host === assignment.host &&
              h.port === assignment.port &&
              h.username === assignment.username,
          )
      if (!target) return
      await openHostSession(target)
    })()
    return () => {
      cancelled = true
    }
  }, [loadHosts, openHostSession])

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
          // 记一笔：这是用户主动断的，别自动重连。
          userClosedRef.current.add(s.id)
          void sessionApi.disconnect(s.id).catch(() => {})
          patchSession(s.id, { status: 'disconnected' })
        },
      },
      {
        id: 'close',
        label: '关闭会话',
        icon: 'x',
        danger: true,
        onSelect: () =>
          askConfirm({
            title: 'CLOSE SESSION',
            message: (
              <>
                关闭会话「<span className="confirm-target">{s.label}</span>」？
                <span className="confirm-hint">
                  连接会断开，远端正在运行的进程可能受影响。
                </span>
              </>
            ),
            confirmLabel: '关闭会话',
            danger: true,
            onConfirm: () => removeSession(s.id),
          }),
      },
    ]
  }, [menu, sessions, handleCloneSession, patchSession, removeSession, askConfirm])

  /* ── 主机右键菜单 ───────────────────────────────── */
  const [hostMenu, setHostMenu] = useState<{
    x: number
    y: number
    hostId: string
  } | null>(null)

  /* ── 分组右键菜单 ───────────────────────────────── */
  const [groupMenu, setGroupMenu] = useState<{
    x: number
    y: number
    name: string
  } | null>(null)

  const groupMenuItems = useMemo<ContextMenuItem[]>(() => {
    if (!groupMenu) return []
    return [
      {
        id: 'connect',
        label: '新建连接',
        icon: 'plus',
        onSelect: () => {
          // 从这个分组右键进来的 —— 新建连接默认就落在这个分组。
          setNewConnGroup(groupMenu.name)
          setEditingHost(null)
          setDialogOpen(true)
        },
      },
    ]
  }, [groupMenu])

  const hostMenuItems = useMemo<ContextMenuItem[]>(() => {
    if (!hostMenu) return []
    const h = hosts.find((x) => x.id === hostMenu.hostId)
    if (!h) return []
    const existing = useSessions
      .getState()
      .findByTarget(h.host, h.port, h.username)
    const items: ContextMenuItem[] = [
      {
        id: 'open',
        label: '开启新会话',
        icon: 'plus',
        onSelect: () => void openHostSession(h),
      },
      {
        id: 'open-window',
        label: '在新窗口打开',
        icon: 'window',
        onSelect: () => void handleOpenInWindow(h),
      },
    ]
    if (existing) {
      items.push({
        id: 'switch',
        label: '切换到已有会话',
        icon: 'terminal',
        onSelect: () => switchToHostSession(h),
      })
    }
    items.push(
      {
        id: 'edit',
        label: '编辑',
        icon: 'edit',
        onSelect: () => {
          setEditingHost(h)
          setDialogOpen(true)
        },
      },
      {
        id: 'delete',
        label: '删除',
        icon: 'trash',
        danger: true,
        onSelect: () =>
          askConfirm({
            title: 'DELETE HOST',
            message: (
              <>
                删除主机「<span className="confirm-target">{h.label}</span>」？
                <span className="confirm-hint">
                  地址、用户名、凭据都会一起删掉，无法恢复。
                </span>
              </>
            ),
            confirmLabel: '删除',
            danger: true,
            onConfirm: () => void removeHost(h.id),
          }),
      },
    )
    return items
  }, [hostMenu, hosts, openHostSession, handleOpenInWindow, switchToHostSession, removeHost, askConfirm])

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
            // 从工具栏新建 —— 不指定分组，按当前选中的主机算。
            setNewConnGroup(null)
            setDialogOpen(true)
          }}
          onOpenNewSession={(host) => void openHostSession(host)}
          onActivateHost={(host) => void handleActivateHost(host)}
          onEditHost={(h) => {
            setEditingHost(h)
            setDialogOpen(true)
          }}
          onHostMenu={(host, x, y) => setHostMenu({ hostId: host.id, x, y })}
          onGroupMenu={(name, x, y) => setGroupMenu({ name, x, y })}
          askConfirm={askConfirm}
        />

        <main className="workspace">
          <SessionTabs
            onSessionMenu={(sessionId, x, y) => setMenu({ sessionId, x, y })}
            onRenameStart={(sessionId) => setRenamingSession(sessionId)}
            renamingId={renamingSession}
            askConfirm={askConfirm}
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
                  // key 用**稳定**的 uid —— 重连会换 session id，
                  // 用 id 的话终端组件会被重建，断线前的屏幕内容全丢。
                  key={s.uid}
                  className={`stage-pane${s.id === activeId ? ' is-active' : ''}`}
                  style={{ visibility: s.id === activeId ? 'visible' : 'hidden' }}
                >
                  {/* 终端一直留在下面 —— 断了也不抹掉屏幕上的内容。 */}
                  <TerminalView sessionId={s.id} active={s.id === activeId} cwd={s.cwd} />
                  {s.status !== 'connected' && (
                    <SessionOverlay
                      session={s}
                      onReconnect={() => void handleReconnect(s.id)}
                      onClose={() => removeSession(s.id)}
                    />
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
            askConfirm={askConfirm}
          />
        )}
      </div>

      {dialogOpen && (
        <ConnectDialog
          // key 变了就重新挂载 —— 表单初值（尤其是默认分组）重新算一遍，
          // 不会把用户已经打过字的字段冲掉。
          // 新建时把分组也算进 key：从不同分组右键进来，落地分组才不一样。
          key={editingHost?.id ?? `new-${newConnGroup ?? ''}`}
          editHost={editingHost}
          initialGroup={newConnGroup ?? undefined}
          onSubmit={handleConnect}
          onSaveOnly={handleSaveOnly}
          onClose={() => {
            setDialogOpen(false)
            setEditingHost(null)
            setNewConnGroup(null)
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

      {hostMenu && hostMenuItems.length > 0 && (
        <ContextMenu
          x={hostMenu.x}
          y={hostMenu.y}
          items={hostMenuItems}
          onClose={() => setHostMenu(null)}
        />
      )}

      {groupMenu && groupMenuItems.length > 0 && (
        <ContextMenu
          x={groupMenu.x}
          y={groupMenu.y}
          items={groupMenuItems}
          onClose={() => setGroupMenu(null)}
        />
      )}

      {settingsOpen && (
        <SettingsPanel onClose={() => setSettingsOpen(false)} askConfirm={askConfirm} />
      )}

      <ConfirmDialog spec={confirm} onClose={() => setConfirm(null)} />

      <Toasts />
    </div>
  )
}
