import { useEffect, useMemo, useState } from 'react'

import { sessionApi } from '../api/bridge'
import type { AuthType, ConnectRequest, SavedHost, TestResult } from '../api/types'
import { useHosts } from '../store/hosts'
import { useUi } from '../store/ui'

import { Icon } from './Icons'

interface Props {
  /** 非空表示编辑模式 */
  editHost?: SavedHost | null
  onSubmit: (req: ConnectRequest) => Promise<void>
  onClose: () => void
}

interface FormState {
  label: string
  group: string
  host: string
  port: string
  username: string
  authType: AuthType
  password: string
  privateKeyPath: string
  passphrase: string
  save: boolean
}

const EMPTY: FormState = {
  label: '',
  group: '',
  host: '',
  port: '22',
  username: 'root',
  authType: 'password',
  password: '',
  privateKeyPath: '',
  passphrase: '',
  save: true,
}

export default function ConnectDialog({ editHost, onSubmit, onClose }: Props) {
  const isEdit = !!editHost
  const [testing, setTesting] = useState(false)
  const [connecting, setConnecting] = useState(false)
  const [testResult, setTestResult] = useState<TestResult | null>(null)
  const [error, setError] = useState<string | null>(null)

  // 已经建过的分组，输入框可以直接选；输入新名字就是新建一个分组。
  const groups = useHosts((s) => s.groups)
  const existingGroups = useMemo(() => {
    const set = new Set<string>()
    for (const g of groups) {
      const name = g.trim()
      if (name) set.add(name)
    }
    return [...set].sort()
  }, [groups])

  /** 当前选中的那台机器在哪个分组 —— 新建连接时默认落在那儿。 */
  const selectedHostId = useUi((s) => s.selectedHostId)
  const allHosts = useHosts((s) => s.hosts)
  const defaultGroup = useMemo(() => {
    const picked = allHosts.find((h) => h.id === selectedHostId)
    return (picked?.group ?? '').trim()
  }, [allHosts, selectedHostId])

  /**
   * 表单初值只算一次。
   * 编辑：带出这台机器自己的分组（可改，清空即移出分组）。
   * 新建：落在当前选中的那台机器所在的分组 —— 顺着上下文往下建，少改一次字段；
   *      没选中任何机器就是 HOSTS（未分组）。
   *
   * 换主机 / 换模式时靠 App.tsx 上的 key 让组件重新挂载、重新算初值 ——
   * 这样用户在字段里打了一半的字不会被中途冲掉。
   */
  const [form, setForm] = useState<FormState>(() => {
    if (!editHost) return { ...EMPTY, group: defaultGroup }
    return {
      label: editHost.label,
      group: editHost.group ?? '',
      host: editHost.host,
      port: String(editHost.port),
      username: editHost.username,
      authType: editHost.authType,
      // 密码不回显：凭据库里的秘密只用于连接，不把明文塞回表单。
      password: '',
      privateKeyPath: editHost.privateKeyPath ?? '',
      passphrase: '',
      save: true,
    }
  })

  // Esc 关闭这个对话框 —— 和右键菜单、设置面板一个规矩。
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose()
    }
    window.addEventListener('keydown', onKey, true)
    return () => window.removeEventListener('keydown', onKey, true)
  }, [onClose])

  const set = <K extends keyof FormState>(k: K, v: FormState[K]) => {
    setForm((f) => ({ ...f, [k]: v }))
    setTestResult(null)
    setError(null)
  }

  const buildRequest = (): ConnectRequest => ({
    label: form.label.trim() || `${form.username}@${form.host}`,
    group: form.group.trim() || undefined,
    host: form.host.trim(),
    port: Number.parseInt(form.port, 10) || 22,
    username: form.username.trim(),
    authType: form.authType,
    password: form.authType === 'password' ? form.password : undefined,
    privateKeyPath: form.authType === 'privateKey' ? form.privateKeyPath.trim() : undefined,
    passphrase: form.authType === 'privateKey' ? form.passphrase : undefined,
    save: form.save,
    hostId: editHost?.id,
  })

  const validate = (): boolean => {
    if (!form.host.trim()) {
      setError('请填写主机地址')
      return false
    }
    if (!form.username.trim()) {
      setError('请填写用户名')
      return false
    }
    if (form.authType === 'privateKey' && !form.privateKeyPath.trim()) {
      setError('请填写私钥路径')
      return false
    }
    const port = Number.parseInt(form.port, 10)
    if (!Number.isInteger(port) || port < 1 || port > 65535) {
      setError('端口需在 1–65535 之间')
      return false
    }
    return true
  }

  const handleTest = async () => {
    if (!validate()) return
    setTesting(true)
    setTestResult(null)
    setError(null)
    try {
      const res = await sessionApi.test(buildRequest())
      setTestResult(res)
    } catch (e) {
      const payload = e as { message?: string; hint?: string }
      setTestResult({
        ok: false,
        error: {
          code: 'INTERNAL',
          message: payload.message ?? String(e),
          hint: payload.hint,
        },
      })
    } finally {
      setTesting(false)
    }
  }

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    if (!validate()) return
    setConnecting(true)
    setError(null)
    try {
      await onSubmit(buildRequest())
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
      setConnecting(false)
    }
  }

  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal" role="dialog" aria-modal="true" aria-label="连接设置">
        <div className="modal-head">
          <span className="meta">{isEdit ? 'EDIT CONNECTION' : 'NEW CONNECTION'}</span>
          <button className="icon-btn" onClick={onClose} aria-label="关闭">
            <Icon name="x" size={13} />
          </button>
        </div>

        <form className="modal-body" onSubmit={handleSubmit}>
          <div className="field">
            <label className="field-label" htmlFor="f-label">
              名称
            </label>
            <input
              id="f-label"
              className="input"
              value={form.label}
              onChange={(e) => set('label', e.target.value)}
              placeholder={`${form.username}@${form.host || 'host'}`}
            />
          </div>

          <div className="field-row">
            <div className="field field-grow">
              <label className="field-label" htmlFor="f-host">
                主机
              </label>
              <input
                id="f-host"
                className="input"
                value={form.host}
                onChange={(e) => set('host', e.target.value)}
                placeholder="192.168.1.1"
                autoFocus
              />
            </div>
            <div className="field field-port">
              <label className="field-label" htmlFor="f-port">
                端口
              </label>
              <input
                id="f-port"
                className="input input-mono"
                value={form.port}
                onChange={(e) => set('port', e.target.value)}
                placeholder="22"
                inputMode="numeric"
              />
            </div>
          </div>

          <div className="field">
            <label className="field-label" htmlFor="f-group">
              分组
            </label>
            <input
              id="f-group"
              className="input"
              value={form.group}
              onChange={(e) => set('group', e.target.value)}
              placeholder="留空不分组；填新名字就是新建分组"
              list="f-group-options"
            />
            <datalist id="f-group-options">
              {existingGroups.map((g) => (
                <option key={g} value={g} />
              ))}
            </datalist>
            <span className="field-note">
              {isEdit
                ? '可改；清空即移出分组'
                : defaultGroup
                  ? `默认在「${defaultGroup}」分组`
                  : '默认在 HOSTS（未分组）'}
            </span>
          </div>

          <div className="field">
            <label className="field-label" htmlFor="f-user">
              用户名
            </label>
            <input
              id="f-user"
              className="input"
              value={form.username}
              onChange={(e) => set('username', e.target.value)}
              placeholder="root"
            />
          </div>

          <div className="field">
            <span className="field-label">认证方式</span>
            <div className="seg">
              {(['password', 'privateKey'] as AuthType[]).map((t) => (
                <button
                  key={t}
                  type="button"
                  className={`seg-item${form.authType === t ? ' is-active' : ''}`}
                  onClick={() => set('authType', t)}
                >
                  <Icon name={t === 'password' ? 'key' : 'file'} size={12} />
                  {t === 'password' ? '密码' : '私钥'}
                </button>
              ))}
            </div>
          </div>

          {form.authType === 'password' ? (
            <div className="field">
              <label className="field-label" htmlFor="f-pass">
                密码
              </label>
              <input
                id="f-pass"
                className="input"
                type="password"
                value={form.password}
                onChange={(e) => set('password', e.target.value)}
                placeholder="••••••••"
                autoComplete="off"
              />
              {isEdit && editHost?.hasPassword && !form.password && (
                <span className="field-note">已使用已保存的密码</span>
              )}
            </div>
          ) : (
            <>
              <div className="field">
                <label className="field-label" htmlFor="f-key">
                  私钥路径
                </label>
                <input
                  id="f-key"
                  className="input input-mono"
                  value={form.privateKeyPath}
                  onChange={(e) => set('privateKeyPath', e.target.value)}
                  placeholder="~/.ssh/id_ed25519"
                  autoComplete="off"
                />
              </div>
              <div className="field">
                <label className="field-label" htmlFor="f-passphrase">
                  私钥口令
                </label>
                <input
                  id="f-passphrase"
                  className="input"
                  type="password"
                  value={form.passphrase}
                  onChange={(e) => set('passphrase', e.target.value)}
                  placeholder="可选"
                  autoComplete="off"
                />
                {isEdit && editHost?.hasPassphrase && !form.passphrase && (
                  <span className="field-note">已使用已保存的口令</span>
                )}
              </div>
            </>
          )}

          {testResult && (
            <div className={`callout ${testResult.ok ? 'is-ok' : 'is-err'}`}>
              <Icon name={testResult.ok ? 'check' : 'alert'} size={12} />
              <div>
                <div className="callout-title">
                  {testResult.ok
                    ? `连接成功 · ${testResult.latencyMs ?? '—'} ms`
                    : (testResult.error?.message ?? '连接失败')}
                </div>
                {testResult.error?.hint && <div className="callout-hint">{testResult.error.hint}</div>}
              </div>
            </div>
          )}

          {error && (
            <div className="callout is-err">
              <Icon name="alert" size={12} />
              <div className="callout-title">{error}</div>
            </div>
          )}

          <label className="checkbox">
            <input
              type="checkbox"
              checked={form.save}
              onChange={(e) => set('save', e.target.checked)}
            />
            <span className="checkbox-box" />
            <span>保存到主机列表（密码加密后存在本机）</span>
          </label>

          <div className="modal-actions">
            <button
              type="button"
              className="btn btn-ghost"
              onClick={handleTest}
              disabled={testing || connecting}
            >
              {testing ? '测试中…' : '测试连接'}
            </button>
            <button type="submit" className="btn btn-primary" disabled={connecting || testing}>
              {connecting ? '连接中…' : isEdit ? '保存并连接' : '连接'}
            </button>
          </div>
        </form>
      </div>
    </div>
  )
}
