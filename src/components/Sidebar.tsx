import { useEffect, useMemo, useRef, useState } from 'react'

import type { SavedHost } from '../api/types'
import { useHosts } from '../store/hosts'
import { useUi } from '../store/ui'
import { Icon } from './Icons'

interface Props {
  onConnect: () => void
  onSelectHost: (host: SavedHost) => void
  /** 强制为该主机新开一个会话（即使已有会话）。 */
  onOpenNewSession: (host: SavedHost) => void
  onEditHost: (host: SavedHost) => void
}

/** 一个分组。`name` 为空表示「未分组」。 */
interface HostGroup {
  name: string
  hosts: SavedHost[]
}

/** 侧边栏里正在编辑分组的方式。 */
type GroupEdit = { mode: 'create' } | { mode: 'rename'; name: string } | null

export default function Sidebar({
  onConnect,
  onSelectHost,
  onOpenNewSession,
  onEditHost,
}: Props) {
  const collapsed = useUi((s) => s.sidebarCollapsed)
  const toggleSidebar = useUi((s) => s.toggleSidebar)
  const filePanelOpen = useUi((s) => s.filePanelOpen)
  const setFilePanel = useUi((s) => s.setFilePanel)
  const push = useUi((s) => s.push)
  const reportError = useUi((s) => s.reportError)

  const hosts = useHosts((s) => s.hosts)
  const registry = useHosts((s) => s.groups)
  const removeHost = useHosts((s) => s.remove)
  const createGroup = useHosts((s) => s.createGroup)
  const renameGroup = useHosts((s) => s.renameGroup)
  const removeGroup = useHosts((s) => s.removeGroup)

  const [hoverHost, setHoverHost] = useState<string | null>(null)
  const [hoverGroup, setHoverGroup] = useState<string | null>(null)
  /** 折叠起来的分组名。 */
  const [collapsedGroups, setCollapsedGroups] = useState<string[]>([])
  /** 正在新建 / 改名的分组。 */
  const [groupEdit, setGroupEdit] = useState<GroupEdit>(null)
  const [groupDraft, setGroupDraft] = useState('')
  const groupInputRef = useRef<HTMLInputElement>(null)
  /** 本轮编辑是否已经提交过（防止 blur 与回车重复提交）。 */
  const committingRef = useRef(false)

  /**
   * 主机按分组归拢。
   *
   * 分组清单（`groups`）是权威：它决定顺序，也允许**空分组**存在 ——
   * 用户可以先把「生产 / 测试」建好，往里放机器时直接选。
   * 清单里没有、但主机上写着的分组（比如老版本留下的）照样显示，
   * 追加在清单后面。
   */
  const groups = useMemo<HostGroup[]>(() => {
    const map = new Map<string, SavedHost[]>()
    for (const name of registry) map.set(name, [])
    for (const h of hosts) {
      const key = (h.group ?? '').trim()
      const bucket = map.get(key)
      if (bucket) bucket.push(h)
      else map.set(key, [h])
    }
    const entries = [...map.entries()]
    // 没分组的排最前，其余按清单顺序（清单外新出现的接在后面）
    const ungrouped = entries.filter(([name]) => name === '')
    const rest = entries.filter(([name]) => name !== '')
    return [...ungrouped, ...rest].map(([name, list]) => ({ name, hosts: list }))
  }, [hosts, registry])

  const toggleGroup = (name: string) =>
    setCollapsedGroups((prev) =>
      prev.includes(name) ? prev.filter((g) => g !== name) : [...prev, name],
    )

  /* ── 分组：新建 / 改名 / 删除 ───────────────────── */
  useEffect(() => {
    if (!groupEdit) return
    const t = window.setTimeout(() => {
      groupInputRef.current?.focus()
      groupInputRef.current?.select()
    }, 0)
    return () => window.clearTimeout(t)
  }, [groupEdit])

  const startCreateGroup = () => {
    committingRef.current = false
    setGroupDraft('')
    setGroupEdit({ mode: 'create' })
  }

  const startRenameGroup = (name: string) => {
    committingRef.current = false
    setGroupDraft(name)
    setGroupEdit({ mode: 'rename', name })
  }

  const commitGroupEdit = () => {
    // 回车提交后输入框会被卸载，可能再触发一次 blur —— 只提交一次。
    if (committingRef.current) return
    committingRef.current = true
    const edit = groupEdit
    const next = groupDraft.trim()
    setGroupEdit(null)
    if (!edit || !next) return

    if (edit.mode === 'create') {
      if (registry.includes(next)) {
        push({ kind: 'info', title: '分组已存在', message: `「${next}」已经在列表里了` })
        return
      }
      void createGroup(next)
        .then(() => {
          push({ kind: 'success', title: '已新建分组', message: next })
        })
        .catch((e) => reportError('新建分组失败', e))
      return
    }

    const old = edit.name
    if (next === old) return
    void renameGroup(old, next)
      .then(() => {
        // 折叠状态跟着改名走，不然组一改名就自动展开了。
        setCollapsedGroups((prev) => prev.map((g) => (g === old ? next : g)))
        push({
          kind: 'success',
          title: '分组已改名',
          message: `「${old}」→「${next}」`,
        })
      })
      .catch((e) => reportError('分组改名失败', e))
  }

  const deleteGroup = (g: HostGroup) => {
    void removeGroup(g.name)
      .then(() => {
        setCollapsedGroups((prev) => prev.filter((x) => x !== g.name))
        push({
          kind: 'info',
          title: '已删除分组',
          message: `「${g.name}」的 ${g.hosts.length} 台主机回到未分组`,
        })
      })
      .catch((e) => reportError('删除分组失败', e))
  }

  if (collapsed) {
    return (
      <aside className="sidebar sidebar-collapsed">
        <button className="icon-btn" onClick={toggleSidebar} title="展开侧边栏">
          <Icon name="menu" size={15} />
        </button>
        <button className="icon-btn icon-btn-accent" onClick={onConnect} title="新建连接">
          <Icon name="plus" size={15} />
        </button>
      </aside>
    )
  }

  const renderHost = (h: SavedHost) => (
    <div
      key={h.id}
      className="host-row"
      onClick={() => onSelectHost(h)}
      onMouseEnter={() => setHoverHost(h.id)}
      onMouseLeave={() => setHoverHost(null)}
    >
      <Icon name="server" size={13} className="host-icon" />
      <div className="host-row-body">
        <div className="host-row-label">{h.label}</div>
        <div className="host-row-meta">
          {h.username}@{h.host}:{h.port}
        </div>
      </div>
      {hoverHost === h.id && (
        <div className="host-row-actions">
          <button
            className="icon-btn icon-btn-accent"
            title="以此主机新开一个会话"
            onClick={(e) => {
              e.stopPropagation()
              onOpenNewSession(h)
            }}
          >
            <Icon name="plus" size={12} />
          </button>
          <button
            className="icon-btn"
            title="编辑"
            onClick={(e) => {
              e.stopPropagation()
              onEditHost(h)
            }}
          >
            <Icon name="edit" size={12} />
          </button>
          <button
            className="icon-btn icon-btn-danger"
            title="删除"
            onClick={(e) => {
              e.stopPropagation()
              void removeHost(h.id)
            }}
          >
            <Icon name="x" size={12} />
          </button>
        </div>
      )}
    </div>
  )

  return (
    <aside className="sidebar">
      <div className="sidebar-head">
        <span className="brand-mark display">SSH7</span>
        <button className="icon-btn" onClick={toggleSidebar} title="收起侧边栏">
          <Icon name="chevron-left" size={14} />
        </button>
      </div>

      <div className="sidebar-tools">
        <button className="tool-btn tool-btn-primary" onClick={onConnect}>
          <Icon name="plus" size={12} />
          <span>连接</span>
        </button>
        <button
          className={`tool-btn${filePanelOpen ? ' is-on' : ''}`}
          onClick={() => setFilePanel(!filePanelOpen)}
          title="切换文件面板"
        >
          <Icon name="folder" size={12} />
          <span>文件</span>
        </button>
      </div>

      <div className="sidebar-section sidebar-section-grow">
        {groups.length === 0 ? (
          <div className="sidebar-empty">
            <Icon name="server" size={18} />
            <span>还没有保存的主机</span>
          </div>
        ) : (
          groups.map((g) => {
            const isCollapsed = collapsedGroups.includes(g.name)
            const isRenaming = groupEdit?.mode === 'rename' && groupEdit.name === g.name
            const title = g.name || 'HOSTS'
            return (
              <div className="sidebar-group" key={g.name || '__none'}>
                <div
                  className={`meta sidebar-section-head sidebar-group-head${isCollapsed ? ' is-collapsed' : ''}`}
                  onClick={() => !isRenaming && toggleGroup(g.name)}
                  onMouseEnter={() => setHoverGroup(g.name)}
                  onMouseLeave={() => setHoverGroup(null)}
                  title={g.name ? '折叠 / 展开分组' : undefined}
                >
                  <span className="sidebar-group-caret">
                    <Icon name="chevron-left" size={11} />
                  </span>
                  {isRenaming ? (
                    <input
                      ref={groupInputRef}
                      className="group-rename"
                      value={groupDraft}
                      onChange={(e) => setGroupDraft(e.target.value)}
                      onClick={(e) => e.stopPropagation()}
                      onDoubleClick={(e) => e.stopPropagation()}
                      onBlur={commitGroupEdit}
                      onKeyDown={(e) => {
                        e.stopPropagation()
                        if (e.key === 'Enter') {
                          e.preventDefault()
                          commitGroupEdit()
                        } else if (e.key === 'Escape') {
                          e.preventDefault()
                          // 取消：标记成已提交，免得随后那声 blur 又把它改回去。
                          committingRef.current = true
                          setGroupEdit(null)
                        }
                      }}
                    />
                  ) : (
                    <span className="sidebar-group-title">{title}</span>
                  )}
                  <span className="sidebar-count">{g.hosts.length}</span>
                  {g.name && !isRenaming && hoverGroup === g.name && (
                    <span className="sidebar-group-actions">
                      <button
                        className="icon-btn"
                        title="重命名分组"
                        onClick={(e) => {
                          e.stopPropagation()
                          startRenameGroup(g.name)
                        }}
                      >
                        <Icon name="edit" size={11} />
                      </button>
                      <button
                        className="icon-btn icon-btn-danger"
                        title="删除分组（里面的主机回到未分组）"
                        onClick={(e) => {
                          e.stopPropagation()
                          deleteGroup(g)
                        }}
                      >
                        <Icon name="trash" size={11} />
                      </button>
                    </span>
                  )}
                </div>
                {!isCollapsed && <div className="sidebar-list">{g.hosts.map(renderHost)}</div>}
              </div>
            )
          })
        )}

        {/* 新建分组 */}
        <div className="sidebar-group-add">
          {groupEdit?.mode === 'create' ? (
            <input
              ref={groupInputRef}
              className="group-rename"
              value={groupDraft}
              placeholder="分组名"
              onChange={(e) => setGroupDraft(e.target.value)}
              onBlur={commitGroupEdit}
              onKeyDown={(e) => {
                e.stopPropagation()
                if (e.key === 'Enter') {
                  e.preventDefault()
                  commitGroupEdit()
                } else if (e.key === 'Escape') {
                  e.preventDefault()
                  committingRef.current = true
                  setGroupEdit(null)
                }
              }}
            />
          ) : (
            <button
              className="meta sidebar-group-add-btn"
              onClick={startCreateGroup}
              title="新建一个分组"
            >
              <Icon name="plus" size={11} />
              新建分组
            </button>
          )}
        </div>
      </div>

      <div className="sidebar-foot">
        <span className="meta">RUST · SSH2 · SFTP</span>
      </div>
    </aside>
  )
}
