import { create } from 'zustand'
import { createJSONStorage, persist, type StateStorage } from 'zustand/middleware'

/** 终端字号的取值范围。 */
export const TERMINAL_FONT_MIN = 9
export const TERMINAL_FONT_MAX = 24
export const TERMINAL_FONT_DEFAULT = 12.5

/** 界面缩放的取值范围。 */
export const UI_SCALE_MIN = 0.85
export const UI_SCALE_MAX = 1.3
export const UI_SCALE_DEFAULT = 1

/** 文件面板宽度的取值范围（px）。 */
export const FILE_PANEL_WIDTH_DEFAULT = 340
export const FILE_PANEL_WIDTH_MIN = 240
export const FILE_PANEL_WIDTH_MAX = 2000

const clamp = (v: number, min: number, max: number) =>
  Math.min(max, Math.max(min, v))

/**
 * 界面偏好设置。
 *
 * 这里只存**显示偏好**，不是任何凭据 —— 密码与私钥口令仍然只进
 * 系统钥匙串，因此用 localStorage 持久化是安全的。
 */
interface SettingsStore {
  /** 终端字号（px） */
  terminalFontSize: number
  /** 界面整体缩放系数 */
  uiScale: number
  /** 文件面板宽度（px）—— 拖动左边缘即可调整。 */
  filePanelWidth: number
  setTerminalFontSize: (size: number) => void
  /** 按步长微调终端字号，用于快捷键 */
  nudgeTerminalFontSize: (delta: number) => void
  setUiScale: (scale: number) => void
  /** 按步长微调界面缩放 */
  nudgeUiScale: (delta: number) => void
  /** 设置文件面板宽度（超出范围的会被夹取）。 */
  setFilePanelWidth: (width: number) => void
  reset: () => void
}

/** 把缩放系数写到根元素，CSS 里的 `--ui-scale` 令牌据此计算字号。 */
export function applyUiScale(scale: number) {
  // 单元测试等无 DOM 环境里直接跳过。
  if (typeof document === 'undefined') return
  document.documentElement.style.setProperty('--ui-scale', String(scale))
}

/**
 * 存储兜底。
 *
 * 正常运行时用的是浏览器 localStorage；但在单元测试这类没有 localStorage
 * 的环境里会抛错，这里退回内存存储，保证 persist 中间件不会因为
 * 存储不可用而整体失效。
 */
const memory = new Map<string, string>()

const rawStorage: StateStorage<void> = {
  getItem: (name) => {
    try {
      return globalThis.localStorage?.getItem(name) ?? null
    } catch {
      return memory.get(name) ?? null
    }
  },
  setItem: (name, value) => {
    try {
      if (globalThis.localStorage) {
        globalThis.localStorage.setItem(name, value)
        return
      }
    } catch {
      /* 落到内存存储 */
    }
    memory.set(name, value)
  },
  removeItem: (name) => {
    try {
      globalThis.localStorage?.removeItem(name)
    } catch {
      /* 忽略 */
    }
    memory.delete(name)
  },
}

/** 落盘的形状：只存这几个数值。 */
export interface PersistedSettings {
  terminalFontSize: number
  uiScale: number
  filePanelWidth: number
}

const storage = createJSONStorage<PersistedSettings>(() => rawStorage)

export const useSettings = create<SettingsStore>()(
  persist<SettingsStore, [], [], PersistedSettings>(
    (set, get) => ({
      terminalFontSize: TERMINAL_FONT_DEFAULT,
      uiScale: UI_SCALE_DEFAULT,
      filePanelWidth: FILE_PANEL_WIDTH_DEFAULT,

      setTerminalFontSize: (size) =>
        set({
          terminalFontSize:
            Math.round(clamp(size, TERMINAL_FONT_MIN, TERMINAL_FONT_MAX) * 2) / 2,
        }),

      nudgeTerminalFontSize: (delta) =>
        set({
          terminalFontSize:
            Math.round(
              clamp(
                get().terminalFontSize + delta,
                TERMINAL_FONT_MIN,
                TERMINAL_FONT_MAX,
              ) * 2,
            ) / 2,
        }),

      setUiScale: (scale) => {
        const next = Math.round(clamp(scale, UI_SCALE_MIN, UI_SCALE_MAX) * 100) / 100
        set({ uiScale: next })
        applyUiScale(next)
      },

      // 从 store 里读当前值再累加，避免连点时用渲染闭包里的旧值。
      nudgeUiScale: (delta) => {
        const next = Math.round(clamp(get().uiScale + delta, UI_SCALE_MIN, UI_SCALE_MAX) * 100) / 100
        set({ uiScale: next })
        applyUiScale(next)
      },

      // 面板宽度：拖动时每帧都会调进来，所以不做过多的处理，只夹范围。
      setFilePanelWidth: (width) =>
        set({
          filePanelWidth: Math.round(
            clamp(width, FILE_PANEL_WIDTH_MIN, FILE_PANEL_WIDTH_MAX),
          ),
        }),

      reset: () => {
        set({
          terminalFontSize: TERMINAL_FONT_DEFAULT,
          uiScale: UI_SCALE_DEFAULT,
          filePanelWidth: FILE_PANEL_WIDTH_DEFAULT,
        })
        applyUiScale(UI_SCALE_DEFAULT)
      },
    }),
    {
      name: 'ssh7.settings',
      storage,
      // 只持久化这几个数值，函数不入盘。
      partialize: (s) => ({
        terminalFontSize: s.terminalFontSize,
        uiScale: s.uiScale,
        filePanelWidth: s.filePanelWidth,
      }),
      onRehydrateStorage: () => (state) => {
        if (state) applyUiScale(state.uiScale)
      },
    },
  ),
)
