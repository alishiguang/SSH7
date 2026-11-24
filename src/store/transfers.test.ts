import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { TRANSFER_DONE_TTL, useTransfers } from './transfers'

beforeEach(() => {
  vi.useFakeTimers()
  useTransfers.setState({ transfers: [] })
})

afterEach(() => {
  vi.useRealTimers()
})

function startOne(id = 't1') {
  useTransfers.getState().start({
    id,
    sessionId: 's1',
    fileName: 'a.txt',
    transferred: 0,
    total: 10,
    direction: 'upload',
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
})
