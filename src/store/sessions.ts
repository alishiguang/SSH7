import { create } from 'zustand'

import type { AppErrorPayload, AuthType, ConnectRequest } from '../api/types'

/** 一次 SSH 会话在前端的状态。 */
export interface SessionState {
  /**
   * 稳定标识 —— **不随重连改变**。
   *
   * 后端每次连接都会发一个新的 session id，重连时也就换了一个。React 的
   * `key` 用它的话，终端组件会被整个重建，屏幕内容全丢。所以另给一个
   * 永远不变的 `uid`：终端靠它保持存活，重连时只是换了个数据源。
   */
  uid: string
  /** 与 Rust 侧 `SessionInfo.sessionId` 一致 */
  id: string
  label: string
  host: string
  port: number
  username: string
  /** 本次使用的认证方式，用于「重试」时复用正确的凭据来源 */
  authType: AuthType
  status: 'connecting' | 'connected' | 'reconnecting' | 'disconnected' | 'failed'
  /** 远端当前工作目录，由 OSC 7 跟踪得到 */
  cwd?: string
  /** 连接失败或中途出错时的结构化错误 */
  error?: AppErrorPayload
  /**
   * 这台主机在凭据库里的 id。
   *
   * **重连能不能成功，全看它** —— 后端拿不到 `hostId` 就没法从加密凭据库
   * 里取回密码（`resolve_credential`），少了它重连必然认证失败。
   */
  hostId?: string
  /** 私钥路径 —— 私钥认证的主机重连时靠它。 */
  privateKeyPath?: string
  /** 自动重连已经尝试了几次（用于界面上的「第 N 次尝试」）。 */
  attempt?: number
}

/**
 * 自动重连的退避表（毫秒）。
 *
 * 第 1 次立刻试（多半是网络抖一下），第 2、3 次各等一会儿再试 ——
 * 服务器重启要时间，等太久也没意义。
 */
export const RECONNECT_DELAYS = [0, 1200, 3000] as const

/**
 * 构造「重新连接」用的请求。
 *
 * **必须带上 `hostId` 和 `privateKeyPath`**：
 *
 * - 少了 `hostId` → 后端没法从加密凭据库取回密码 → `AuthFailed`；
 * - 少了 `privateKeyPath` → 私钥认证的主机连私钥都找不到 → `InvalidInput`。
 *
 * 旧的「重试」按钮两个都没带，所以**每次重连都是失败的**（缺陷 #43）。
 */
export function buildReconnectRequest(s: Omit<SessionState, 'uid'>): ConnectRequest {
  return {
    label: s.label,
    host: s.host,
    port: s.port,
    username: s.username,
    authType: s.authType,
    hostId: s.hostId,
    privateKeyPath: s.privateKeyPath,
    // 重连不重新保存主机 —— 它已经在列表里了。
    save: false,
  }
}

interface SessionsStore {
  sessions: SessionState[]
  activeId: string | null
  /** 新建一个「连接中」的占位会话（`uid` 由 store 生成）。 */
  begin: (s: Omit<SessionState, 'status' | 'uid'>) => void
  patch: (id: string, patch: Partial<SessionState>) => void
  /**
   * 用真实会话替换占位会话（连接建立后调用）。
   *
   * **`uid` 保持不变** —— 它是终端组件的存活依据，重连时不能换。
   */
  replace: (oldId: string, next: Omit<SessionState, 'uid'>) => void
  /**
   * 把会话挪到新位置（标签栏拖动排序）。
   *
   * `toIndex` 是**挪动之后**这个会话在标签栏里的最终下标。
   * 超界会被夹到合法范围内，所以传 3 给三个标签也安全。
   */
  move: (id: string, toIndex: number) => void
  remove: (id: string) => void
  setActive: (id: string | null) => void
  active: () => SessionState | undefined
  /**
   * 查找已经存在（或正在建立的）同一目标主机会话。
   * 用于「点击已保存主机时不再重复开新会话」。
   */
  findByTarget: (host: string, port: number, username: string) => SessionState | undefined
}

export const useSessions = create<SessionsStore>((set, get) => ({
  sessions: [],
  activeId: null,

  begin: (s) =>
    set((prev) => ({
      sessions: [
        ...prev.sessions,
        { ...s, uid: crypto.randomUUID(), status: 'connecting' },
      ],
      activeId: s.id,
    })),

  patch: (id, patch) =>
    set((prev) => ({
      sessions: prev.sessions.map((s) => (s.id === id ? { ...s, ...patch } : s)),
    })),

  /**
   * 用真实会话替换占位会话。
   *
   * 连接建立之前先摆一个「正在连接」的占位会话（用户马上有反馈），
   * 拿到后端给的真实 session id 之后再换过去 —— 标签页不闪。
   */
  replace: (oldId, next) =>
    set((prev) => {
      const idx = prev.sessions.findIndex((s) => s.id === oldId)
      if (idx < 0) return prev
      const sessions = [...prev.sessions]
      // `uid` 沿用原来的 —— 它是终端组件的存活依据，重连时不能换，
      // 否则终端会被重建，屏幕上的内容全丢。
      sessions[idx] = { ...next, uid: sessions[idx].uid }
      return {
        sessions,
        // 只有占位会话本来是当前标签时才跟着切过去，
        // 别把用户已经切走的焦点抢回来。
        activeId: prev.activeId === oldId ? next.id : prev.activeId,
      }
    }),

  /** 拖动排序：把会话挪到 `toIndex` 这个最终位置。 */
  move: (id, toIndex) =>
    set((prev) => {
      const from = prev.sessions.findIndex((s) => s.id === id)
      if (from < 0) return prev
      const sessions = [...prev.sessions]
      const [item] = sessions.splice(from, 1)
      // 先摘出来再插入，`toIndex` 就是它最终落在哪。
      const idx = Math.max(0, Math.min(toIndex, sessions.length))
      sessions.splice(idx, 0, item)
      return { sessions }
    }),

  remove: (id) =>
    set((prev) => {
      const next = prev.sessions.filter((s) => s.id !== id)
      return {
        sessions: next,
        // 关掉当前标签时，自动落到最后一个还开着的标签。
        activeId:
          prev.activeId === id
            ? next.length > 0
              ? next[next.length - 1].id
              : null
            : prev.activeId,
      }
    }),

  setActive: (id) => set({ activeId: id }),

  active: () => get().sessions.find((s) => s.id === get().activeId),

  findByTarget: (host, port, username) =>
    get().sessions.find(
      (s) =>
        s.host === host &&
        s.port === port &&
        s.username === username &&
        s.status !== 'disconnected' &&
        s.status !== 'failed',
    ),
}))
