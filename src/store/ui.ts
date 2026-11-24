import { create } from 'zustand'

import type { AppErrorPayload } from '../api/types'

/** 一条界面通知。 */
export interface Toast {
  id: string
  kind: 'info' | 'success' | 'error'
  title: string
  message?: string
  hint?: string
}

interface UiStore {
  toasts: Toast[]
  sidebarCollapsed: boolean
  filePanelOpen: boolean
  push: (t: Omit<Toast, 'id'>) => void
  dismiss: (id: string) => void
  toggleSidebar: () => void
  setFilePanel: (open: boolean) => void
  /** 把结构化错误转成通知 */
  reportError: (title: string, e: AppErrorPayload | Error) => void
}

let toastSeq = 0

export const useUi = create<UiStore>((set, get) => ({
  toasts: [],
  sidebarCollapsed: false,
  filePanelOpen: false,

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

  reportError: (title, e) => {
    const payload: AppErrorPayload =
      'code' in e
        ? (e as AppErrorPayload)
        : { code: 'INTERNAL', message: (e as Error).message }
    get().push({
      kind: 'error',
      title,
      message: payload.message,
      hint: payload.hint,
    })
  },
}))
