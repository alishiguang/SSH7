import { create } from 'zustand'

import type { AppErrorPayload } from '../api/types'

/** 一条界面通知。 */
export interface Toast {
  id: string
  kind: 'info' | 'success' | 'error'
  title: string
  message?: string
  hint?: string
  /** 技术细节，小字展示，排查问题时用。 */
  detail?: string
}

interface UiStore {
  toasts: Toast[]
  sidebarCollapsed: boolean
  filePanelOpen: boolean
  /** 侧边栏里当前选中的主机（单击只选中，不建立连接）。 */
  selectedHostId: string | null
  /** 正在改名的会话 id（null 表示没有）。改名期间终端不许抢焦点。 */
  renamingSessionId: string | null
  /** 确认框是否开着。开着时下层对话框的 Esc 让位给它。 */
  confirmOpen: boolean
  push: (t: Omit<Toast, 'id'>) => void
  dismiss: (id: string) => void
  toggleSidebar: () => void
  setFilePanel: (open: boolean) => void
  setSelectedHost: (id: string | null) => void
  setRenamingSession: (id: string | null) => void
  setConfirmOpen: (open: boolean) => void
  /** 把结构化错误转成通知 */
  reportError: (title: string, e: AppErrorPayload | Error) => void
}

let toastSeq = 0

export const useUi = create<UiStore>((set, get) => ({
  toasts: [],
  sidebarCollapsed: false,
  filePanelOpen: false,
  selectedHostId: null,
  renamingSessionId: null,
  confirmOpen: false,

  push: (t) => {
    const id = `toast-${++toastSeq}`
    set((prev) => ({ toasts: [...prev.toasts, { ...t, id }] }))
    // 成功提示 2.4s 后自动消失；错误提示需要用户确认，保留 6s。
    const ttl = t.kind === 'error' ? 6000 : 2400
    window.setTimeout(() => get().dismiss(id), ttl)
  },

  dismiss: (id) =>
    set((prev) => ({ toasts: prev.toasts.filter((t) => t.id !== id) })),

  toggleSidebar: () => set((prev) => ({ sidebarCollapsed: !prev.sidebarCollapsed })),

  setFilePanel: (open) => set({ filePanelOpen: open }),

  setSelectedHost: (id) => set({ selectedHostId: id }),

  setRenamingSession: (id) => set({ renamingSessionId: id }),

  setConfirmOpen: (open) => set({ confirmOpen: open }),

  reportError: (title, e) => {
    // Rust 侧的结构化载荷带 code/detail；普通 Error 至少能拿到 message。
    const payload: AppErrorPayload =
      'code' in e
        ? (e as AppErrorPayload)
        : {
            code: 'INTERNAL',
            message: (e as Error).message,
            detail: 'detail' in e ? String((e as { detail?: unknown }).detail) : undefined,
          }
    get().push({
      kind: 'error',
      title,
      message: payload.message,
      hint: payload.hint,
      detail: payload.detail,
    })
  },
}))
