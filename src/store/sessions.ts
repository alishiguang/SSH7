import { create } from 'zustand'

import type { AppErrorPayload, AuthType } from '../api/types'

/** 一次 SSH 会话在前端的状态。 */
export interface SessionState {
  /** 与 Rust 侧 `SessionInfo.sessionId` 一致 */
  id: string
  label: string
  host: string
  port: number
  username: string
  /** 本次使用的认证方式，用于「重试」时复用正确的凭据来源 */
  authType: AuthType
  status: 'connecting' | 'connected' | 'disconnected' | 'failed'
  /** 远端当前工作目录，由 OSC 7 跟踪得到 */
  cwd?: string
  /** 连接失败或中途出错时的结构化错误 */
  error?: AppErrorPayload
}

interface SessionsStore {
  sessions: SessionState[]
  activeId: string | null
  /** 新建一个「连接中」的占位会话 */
  begin: (s: Omit<SessionState, 'status'>) => void
  patch: (id: string, patch: Partial<SessionState>) => void
  /** 用真实会话替换占位会话（连接建立后调用）。 */
  replace: (oldId: string, next: SessionState) => void
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
   * 查找已经存在���（或正在建立的）同一目标主机会话。
   * 用于「点击已保存主机时不再重复开新会话」。
   */
  findByTarget: (host: string, port: number, username: string) => SessionState | undefined
}

export const useSessions = create<SessionsStore>((set, get) => ({
  sessions: [],
  activeId: null,

  begin: (s) =>
    set((prev) => ({
      sessions: [...prev.sessions, { ...s, status: 'connecting' }],
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
      sessions[idx] = next
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
