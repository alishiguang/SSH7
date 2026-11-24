import { beforeEach, describe, expect, it } from 'vitest'

import {
  TERMINAL_FONT_DEFAULT,
  TERMINAL_FONT_MAX,
  TERMINAL_FONT_MIN,
  UI_SCALE_DEFAULT,
  UI_SCALE_MAX,
  UI_SCALE_MIN,
  useSettings,
} from './settings'

beforeEach(() => {
  useSettings.setState({
    terminalFontSize: TERMINAL_FONT_DEFAULT,
    uiScale: UI_SCALE_DEFAULT,
  })
})

describe('终端字号', () => {
  it('默认是 12.5px', () => {
    expect(useSettings.getState().terminalFontSize).toBe(12.5)
  })

  it('设置后立即生效', () => {
    useSettings.getState().setTerminalFontSize(16)
    expect(useSettings.getState().terminalFontSize).toBe(16)
  })

  it('超出范围会被夹取', () => {
    useSettings.getState().setTerminalFontSize(999)
    expect(useSettings.getState().terminalFontSize).toBe(TERMINAL_FONT_MAX)

    useSettings.getState().setTerminalFontSize(1)
    expect(useSettings.getState().terminalFontSize).toBe(TERMINAL_FONT_MIN)
  })

  it('按 0.5px 取整', () => {
    useSettings.getState().setTerminalFontSize(14.3)
    expect(useSettings.getState().terminalFontSize).toBe(14.5)
  })

  it('快捷键微调按 1px 步进并在边界停下', () => {
    useSettings.getState().nudgeTerminalFontSize(1)
    expect(useSettings.getState().terminalFontSize).toBe(13.5)

    useSettings.getState().nudgeTerminalFontSize(-1)
    expect(useSettings.getState().terminalFontSize).toBe(12.5)

    useSettings.getState().setTerminalFontSize(TERMINAL_FONT_MIN)
    useSettings.getState().nudgeTerminalFontSize(-1)
    expect(useSettings.getState().terminalFontSize).toBe(TERMINAL_FONT_MIN)

    useSettings.getState().setTerminalFontSize(TERMINAL_FONT_MAX)
    useSettings.getState().nudgeTerminalFontSize(1)
    expect(useSettings.getState().terminalFontSize).toBe(TERMINAL_FONT_MAX)
  })
})

describe('界面缩放', () => {
  it('默认 100%', () => {
    expect(useSettings.getState().uiScale).toBe(1)
  })

  it('范围被夹取在 85%~130%', () => {
    useSettings.getState().setUiScale(2)
    expect(useSettings.getState().uiScale).toBe(UI_SCALE_MAX)

    useSettings.getState().setUiScale(0.1)
    expect(useSettings.getState().uiScale).toBe(UI_SCALE_MIN)
  })

  it('按 0.05 取整', () => {
    useSettings.getState().setUiScale(1.234)
    expect(useSettings.getState().uiScale).toBe(1.23)
  })

  it('连点时按步长累加，不会因为渲染闭包里的旧值只生效一次', () => {
    useSettings.getState().nudgeUiScale(0.05)
    useSettings.getState().nudgeUiScale(0.05)
    expect(useSettings.getState().uiScale).toBe(1.1)

    useSettings.getState().nudgeTerminalFontSize(1)
    useSettings.getState().nudgeTerminalFontSize(1)
    expect(useSettings.getState().terminalFontSize).toBe(14.5)
  })
})

describe('恢复默认', () => {
  it('把两个值都还原', () => {
    useSettings.getState().setTerminalFontSize(20)
    useSettings.getState().setUiScale(1.2)
    useSettings.getState().reset()

    expect(useSettings.getState().terminalFontSize).toBe(TERMINAL_FONT_DEFAULT)
    expect(useSettings.getState().uiScale).toBe(UI_SCALE_DEFAULT)
  })
})
