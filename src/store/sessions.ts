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
