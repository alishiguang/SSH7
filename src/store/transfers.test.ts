import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { TRANSFER_DONE_TTL, transfersFor, useTransfers } from './transfers'

beforeEach(() => {
  vi.useFakeTimers()
  useTransfers.setState({ transfers: [] })
})

afterEach(() => {
  vi.useRealTimers()
})

function startOne(id = 't1', origin: 'panel' | 'terminal' = 'panel') {
  useTransfers.getState().start({
    id,
    sessionId: 's1',
    fileName: 'a.txt',
    transferred: 0,
    total: 10,
    direction: 'upload',
    origin,
  })
}

describe('传输提示', () => {
  it('成功后 3 秒自动消失', () => {
    startOne()
    useTransfers.getState().finish('t1')
    expect(useTransfers.getState().transfers).toHaveLength(1)

    // 还没到 3 秒 —— 提示还在。
    vi.advanceTimersByTime(TRANSFER_DONE_TTL - 1)
    expect(useTransfers.getState().transfers).toHaveLength(1)

    // 过了 3 秒 —— 自动收掉。
    vi.advanceTimersByTime(2)
    expect(useTransfers.getState().transfers).toHaveLength(0)
  })

  it('失败的不自动消失 —— 用户得看得见错误', () => {
    startOne()
    useTransfers.getState().finish('t1', '权限不足')
    vi.advanceTimersByTime(TRANSFER_DONE_TTL * 3)
    const t = useTransfers.getState().transfers
    expect(t).toHaveLength(1)
    expect(t[0].status).toBe('error')
    expect(t[0].error).toBe('权限不足')
  })

  it('只收掉结束的那条，进行中的不受影响', () => {
    startOne('t1')
    startOne('t2')
    useTransfers.getState().finish('t1')
    vi.advanceTimersByTime(TRANSFER_DONE_TTL + 1)
    const t = useTransfers.getState().transfers
    expect(t).toHaveLength(1)
    expect(t[0].id).toBe('t2')
    expect(t[0].status).toBe('active')
  })

  it('完成后进度是 100%', () => {
    startOne()
    useTransfers.getState().progress('t1', 5, 10, 50)
    useTransfers.getState().finish('t1')
    expect(useTransfers.getState().transfers[0].percent).toBe(100)
  })

  /* ── 显示位置：谁发起的就只显示在哪一边 ──────────────
     之前两边都按 sessionId 过滤，同一次传输在终端和文件面板各出现一张卡，
     同一件事说两遍。 */
  it('面板发起的传输只在文件面板显示', () => {
    startOne('t1', 'panel')
    startOne('t2', 'terminal')
    const all = useTransfers.getState().transfers

    expect(transfersFor(all, 's1', 'panel').map((t) => t.id)).toEqual(['t1'])
    expect(transfersFor(all, 's1', 'terminal').map((t) => t.id)).toEqual(['t2'])
  })

  it('别的会话的传输不显示', () => {
    useTransfers.getState().start({
      id: 'other',
      sessionId: 's2',
      fileName: 'b.txt',
      transferred: 0,
      total: 10,
      direction: 'download',
      origin: 'panel',
    })
    const all = useTransfers.getState().transfers

    expect(transfersFor(all, 's1', 'panel')).toHaveLength(0)
    expect(transfersFor(all, 's2', 'panel')).toHaveLength(1)
  })

  it('传输来源会记进状态里', () => {
    startOne('t1', 'terminal')
    expect(useTransfers.getState().transfers[0].origin).toBe('terminal')
  })
})
