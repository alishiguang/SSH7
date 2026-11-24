import { create } from 'zustand'

import { getCurrentWebview } from '@tauri-apps/api/webview'

/**
 * 原生文件拖放。
 *
 * Tauri v2 里 `DataTransfer.files[].path` 已被移除，可靠的做法是
 * 监听 WebView 的原生拖放事件，它给出真实本地路径。
 *
 * 全应用只注册这一个监听器 —— 旧版每打开一次文件面板就叠加一个
 * `onSftpProgress` 监听器且从不移除（缺陷 #7）。
 */

/** 一次「松手」事件。 */
export interface DropEvent {
  /** 自增序号，用来识别是不是同一次拖放 */
  seq: number
  /** 拖入的本地路径 */
  paths: string[]
  /** 松手时的光标位置 */
  position: { x: number; y: number }
}

interface DropState {
  /** 当前是否正在拖放 */
  dragging: boolean
  /** 拖入的本地路径列表（悬停阶段，用于预览） */
  paths: string[]
  /** 光标位置（CSS 像素，已按窗口缩放比换算） */
  position: { x: number; y: number } | null
  /**
   * 最近一次**松手**事件。
   *
   * 上传只在松手时触发 —— 旧实现把「悬停」当成了「松手」，
   * 文件还没放手就开传了，拖到别处松手也已经传上去。
   */
  dropped: DropEvent | null
  /** 是否已经初始化过监听器 */
  initialized: boolean
  init: () => Promise<void>
  setDragging: (dragging: boolean, paths?: string[]) => void
  setPosition: (position: { x: number; y: number } | null) => void
}

let dropSeq = 0

export const useDrop = create<DropState>((set, get) => ({
  dragging: false,
  paths: [],
  position: null,
  dropped: null,
  initialized: false,

  init: async () => {
    if (get().initialized) return
    set({ initialized: true })

    try {
      const { getCurrentWindow } = await import('@tauri-apps/api/window')
      const scale = await getCurrentWindow().scaleFactor()

      await getCurrentWebview().onDragDropEvent((event) => {
        const p = event.payload
        switch (p.type) {
          case 'enter':
            set({
              dragging: true,
              paths: p.paths,
              position: { x: p.position.x / scale, y: p.position.y / scale },
            })
            break
          case 'over':
            set({
              dragging: true,
              position: { x: p.position.x / scale, y: p.position.y / scale },
            })
            break
          case 'drop':
            // 只有真正松手才算一次拖放完成。
            set({
              dragging: false,
              paths: [],
              position: null,
              dropped: {
                seq: ++dropSeq,
                paths: p.paths,
                position: { x: p.position.x / scale, y: p.position.y / scale },
              },
            })
            break
          case 'leave':
            set({ dragging: false, paths: [], position: null })
            break
        }
      })
    } catch (e) {
      // 非 Tauri 环境（例如用浏览器预览前端）没有原生拖放能力。
      console.warn('原生拖放不可用', e)
    }
  },

  setDragging: (dragging, paths = []) => set({ dragging, paths }),

  setPosition: (position) => set({ position }),
}))

/**
 * 判断光标是否落在某个元素内。用于把一次拖放路由到正确的区域。
 */
export function isPointInside(
  el: HTMLElement | null,
  point: { x: number; y: number } | null,
): boolean {
  if (!el || !point) return false
  const r = el.getBoundingClientRect()
  return point.x >= r.left && point.x <= r.right && point.y >= r.top && point.y <= r.bottom
}
