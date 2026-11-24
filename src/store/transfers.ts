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

interface TransfersStore {
  transfers: Transfer[]
  start: (t: Omit<Transfer, 'percent' | 'status'>) => void
  progress: (id: string, transferred: number, total: number, percent: number) => void
  finish: (id: string, error?: string) => void
  clear: (id: string) => void
  clearFinished: () => void
}

export const useTransfers = create<TransfersStore>((set) => ({
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

  finish: (id, error) =>
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
    })),

  clear: (id) =>
    set((prev) => ({
      transfers: prev.transfers.filter((t) => t.id !== id),
    })),

  clearFinished: () =>
    set((prev) => ({
      transfers: prev.transfers.filter((t) => t.status === 'active'),
    })),
}))
