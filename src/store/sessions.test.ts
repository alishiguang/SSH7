import { beforeEach, describe, expect, it } from 'vitest'

import { useSessions } from './sessions'

const base = {
  id: 's1',
  label: '生产服务器',
  host: '10.0.0.1',
  port: 22,
  username: 'root',
  authType: 'password' as const,
}

beforeEach(() => {
  useSessions.setState({ sessions: [], activeId: null })
})

describe('useSessions.begin', () => {
  it('以 connecting 状态加入会话并自动激活', () => {
    useSessions.getState().begin(base)
    const { sessions, activeId } = useSessions.getState()
    expect(sessions).toHaveLength(1)
    expect(sessions[0].status).toBe('connecting')
    expect(activeId).toBe('s1')
  })
})

describe('useSessions.findByTarget', () => {
  it('命中同一 host/port/username 的活跃会话', () => {
    useSessions.getState().begin(base)
    useSessions.getState().patch('s1', { status: 'connected' })
    expect(useSessions.getState().findByTarget('10.0.0.1', 22, 'root')?.id).toBe('s1')
  })

  it('连接中的会话同样算作已存在，避免重复开新会话', () => {
    useSessions.getState().begin(base)
    expect(useSessions.getState().findByTarget('10.0.0.1', 22, 'root')?.id).toBe('s1')
  })

  it('端口或用户名不同时不算同一目标', () => {
    useSessions.getState().begin(base)
    expect(useSessions.getState().findByTarget('10.0.0.1', 2222, 'root')).toBeUndefined()
    expect(useSessions.getState().findByTarget('10.0.0.1', 22, 'ops')).toBeUndefined()
  })

  it('已断开或失败的会话不再阻挡新会话', () => {
    useSessions.getState().begin(base)
    useSessions.getState().patch('s1', { status: 'disconnected' })
    expect(useSessions.getState().findByTarget('10.0.0.1', 22, 'root')).toBeUndefined()

    useSessions.getState().patch('s1', { status: 'failed' })
    expect(useSessions.getState().findByTarget('10.0.0.1', 22, 'root')).toBeUndefined()
  })
})

describe('useSessions.patch', () => {
  it('只更新目标会话', () => {
    useSessions.getState().begin(base)
    useSessions.getState().begin({ ...base, id: 's2', label: '生产服务器 · 副本' })
    useSessions.getState().patch('s2', { status: 'connected', cwd: '/srv' })

    const { sessions } = useSessions.getState()
    expect(sessions.find((s) => s.id === 's1')?.status).toBe('connecting')
    expect(sessions.find((s) => s.id === 's2')).toMatchObject({
      status: 'connected',
      cwd: '/srv',
    })
  })
})

describe('useSessions.remove', () => {
  it('移除会话并把焦点落到剩余的最后一个会话', () => {
    useSessions.getState().begin(base)
    useSessions.getState().begin({ ...base, id: 's2' })
    useSessions.getState().remove('s2')

    const { sessions, activeId } = useSessions.getState()
    expect(sessions.map((s) => s.id)).toEqual(['s1'])
    expect(activeId).toBe('s1')
  })

  it('移除最后一个会话后没有焦点', () => {
    useSessions.getState().begin(base)
    useSessions.getState().remove('s1')
    expect(useSessions.getState().sessions).toHaveLength(0)
    expect(useSessions.getState().activeId).toBeNull()
  })
})

describe('useSessions.setActive / active', () => {
  it('切换焦点会话', () => {
    useSessions.getState().begin(base)
    useSessions.getState().begin({ ...base, id: 's2' })
    useSessions.getState().setActive('s1')

    expect(useSessions.getState().active()?.id).toBe('s1')
    expect(useSessions.getState().activeId).toBe('s1')
  })
})
