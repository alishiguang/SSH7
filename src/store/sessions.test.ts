import { beforeEach, describe, expect, it } from 'vitest'

import { buildReconnectRequest, RECONNECT_DELAYS, useSessions } from './sessions'

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

describe('useSessions.move', () => {
  beforeEach(() => {
    useSessions.getState().begin(base)
    useSessions.getState().begin({ ...base, id: 's2' })
    useSessions.getState().begin({ ...base, id: 's3' })
  })

  it('往后拖：拖到最后', () => {
    useSessions.getState().move('s1', 2)
    expect(useSessions.getState().sessions.map((s) => s.id)).toEqual(['s2', 's3', 's1'])
  })

  it('往前拖：插到最前面', () => {
    useSessions.getState().move('s3', 0)
    expect(useSessions.getState().sessions.map((s) => s.id)).toEqual(['s3', 's1', 's2'])
  })

  it('拖到中间', () => {
    useSessions.getState().move('s1', 1)
    expect(useSessions.getState().sessions.map((s) => s.id)).toEqual(['s2', 's1', 's3'])
  })

  it('下标超出范围时夹到末尾', () => {
    useSessions.getState().move('s1', 99)
    expect(useSessions.getState().sessions.map((s) => s.id)).toEqual(['s2', 's3', 's1'])
  })

  it('拖到自己原来的位置不动', () => {
    useSessions.getState().move('s2', 1)
    expect(useSessions.getState().sessions.map((s) => s.id)).toEqual(['s1', 's2', 's3'])
  })

  it('不存在的会话不报错', () => {
    useSessions.getState().move('nope', 0)
    expect(useSessions.getState().sessions.map((s) => s.id)).toEqual(['s1', 's2', 's3'])
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

describe('useSessions.replace', () => {
  it('用真实会话原地替换占位会话，标签页不闪', () => {
    useSessions.getState().begin({ ...base, id: 'pending-1' })
    useSessions.getState().replace('pending-1', { ...base, id: 'real-1', status: 'connected' })

    const { sessions, activeId } = useSessions.getState()
    expect(sessions).toHaveLength(1)
    expect(sessions[0]).toMatchObject({ id: 'real-1', status: 'connected' })
    expect(activeId).toBe('real-1')
  })

  it('用户已经切走焦点时不抢焦点', () => {
    useSessions.getState().begin({ ...base, id: 'pending-1' })
    useSessions.getState().begin({ ...base, id: 's2' })
    useSessions.getState().setActive('s2')
    useSessions.getState().replace('pending-1', { ...base, id: 'real-1', status: 'connected' })

    expect(useSessions.getState().activeId).toBe('s2')
  })

  it('占位会话不在了就不动', () => {
    useSessions.getState().begin(base)
    useSessions.getState().replace('pending-x', { ...base, id: 'real-1', status: 'connected' })

    const { sessions } = useSessions.getState()
    expect(sessions).toHaveLength(1)
    expect(sessions[0].id).toBe('s1')
  })
})

/**
 * 缺陷 #43 的回归测试。
 *
 * 旧的「重试」按钮没带 `hostId` 和 `privateKeyPath` —— 后端取不到凭据，
 * 所以**每次重连都是失败的**。
 */
describe('buildReconnectRequest', () => {
  const withCreds = {
    ...base,
    status: 'disconnected' as const,
    hostId: 'h-123',
    privateKeyPath: '/Users/me/.ssh/id_ed25519',
  }

  it('带上凭据来源 —— 否则后端取不回密码', () => {
    const req = buildReconnectRequest(withCreds)
    expect(req.hostId).toBe('h-123')
    expect(req.privateKeyPath).toBe('/Users/me/.ssh/id_ed25519')
  })

  it('连接参数原样带过去', () => {
    const req = buildReconnectRequest(withCreds)
    expect(req.host).toBe('10.0.0.1')
    expect(req.port).toBe(22)
    expect(req.username).toBe('root')
    expect(req.authType).toBe('password')
    expect(req.label).toBe('生产服务器')
  })

  it('重连不重新保存主机', () => {
    expect(buildReconnectRequest(withCreds).save).toBe(false)
  })

  it('没记下 hostId 时也不会瞎编一个', () => {
    const req = buildReconnectRequest({ ...base, status: 'disconnected' })
    expect(req.hostId).toBeUndefined()
  })
})

describe('RECONNECT_DELAYS', () => {
  it('第一次立刻试，之后逐步退避', () => {
    expect(RECONNECT_DELAYS[0]).toBe(0)
    for (let i = 1; i < RECONNECT_DELAYS.length; i++) {
      expect(RECONNECT_DELAYS[i]).toBeGreaterThan(RECONNECT_DELAYS[i - 1])
    }
  })
})
