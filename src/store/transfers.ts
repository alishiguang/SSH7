import { create } from 'zustand'

/** 一个进行中的文件传输。 */
export interface Transfer {
  /** 由 Rust 端生成，唯一标识一次传输（缺陷 #6：不再用文件名做标识） */
  id: string
  sessionId: string
  fileName: string
  transferred: number
  total: number
  /** 0..100 */
  percent: number
  direction: 'upload' | 'download'
  status: 'active' | 'done' | 'error'
  error?: string
}

/**
 * 传输成功后，条目停留多久就自动消失（ms）。
 *
 * 进度条只是「知会一声」—— 传完了还一直占着屏幕角落，是噪音。
 * 失败的**不自动收**：用户得看得见错误消息。
 */
export const TRANSFER_DONE_TTL = 3000

interface TransfersStore {
  transfers: Transfer[]
  start: (t: Omit<Transfer, 'percent' | 'status'>) => void
  progress: (id: string, transferred: number, total: number, percent: number) => void
  finish: (id: string, error?: string) => void
  clear: (id: string) => void
  clearFinished: () => void
}

export const useTransfers = create<TransfersStore>((set, get) => ({
  transfers: [],

  start: (t) =>
    set((prev) => ({
      transfers: [
        ...prev.transfers,
        { ...t, percent: 0, status: 'active' },
      ],
    })),

  progress: (id, transferred, total, percent) =>
    set((prev) => ({
      transfers: prev.transfers.map((t) =>
        t.id === id ? { ...t, transferred, total, percent } : t,
      ),
    })),

  finish: (id, error) => {
    set((prev) => ({
      transfers: prev.transfers.map((t) =>
        t.id === id
          ? {
              ...t,
              status: error ? 'error' : 'done',
              percent: error ? t.percent : 100,
              error,
            }
          : t,
      ),
    }))
    // 传完了就自动收掉这条提示。
    if (!error) {
      setTimeout(() => get().clear(id), TRANSFER_DONE_TTL)
    }
  },

  clear: (id) =>
    set((prev) => ({
      transfers: prev.transfers.filter((t) => t.id !== id),
    })),

  clearFinished: () =>
    set((prev) => ({
      transfers: prev.transfers.filter((t) => t.status === 'active'),
    })),
}))
