import { useMemo, useState } from 'react'

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

  const hosts = useHosts((s) => s.hosts)
  const removeHost = useHosts((s) => s.remove)

  const [hoverHost, setHoverHost] = useState<string | null>(null)
  /** 折叠起来的分组名。 */
  const [collapsedGroups, setCollapsedGroups] = useState<string[]>([])

  /**
   * 主机按分组归拢。
   *
   * 没有分组的排在最前面（就是原来那个 `// HOSTS` 区），
   * 其余分组按第一次出现的先后排列 —— 用户自己添加主机的顺序就是分组顺序。
   */
  const groups = useMemo<HostGroup[]>(() => {
    const map = new Map<string, SavedHost[]>()
    for (const h of hosts) {
      const key = (h.group ?? '').trim()
      const bucket = map.get(key)
      if (bucket) bucket.push(h)
      else map.set(key, [h])
    }
    const ordered = [...map.entries()]
    // 没分组的排最前，其余按首次出现顺序
      .sort(([a], [b]) => (a === '' ? -1 : b === '' ? 1 : 0))
      .map(([name, list]) => ({ name, hosts: list }))
    return ordered
  }, [hosts])

  const toggleGroup = (name: string) =>
    setCollapsedGroups((prev) =>
      prev.includes(name) ? prev.filter((g) => g !== name) : [...prev, name],
    )

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
        {hosts.length === 0 ? (
          <>
            <div className="meta sidebar-section-head">
              HOSTS <span className="sidebar-count">{hosts.length}</span>
            </div>
            <div className="sidebar-empty">
              <Icon name="server" size={18} />
              <span>还没有保存的主机</span>
            </div>
          </>
        ) : (
          groups.map((g) => {
            const isCollapsed = collapsedGroups.includes(g.name)
            const title = g.name || 'HOSTS'
            return (
              <div className="sidebar-group" key={g.name || '__none'}>
                <div
                  className={`meta sidebar-section-head sidebar-group-head${isCollapsed ? ' is-collapsed' : ''}`}
                  onClick={() => toggleGroup(g.name)}
                  title={g.name ? '折叠 / 展开分组' : undefined}
                >
                  <span className="sidebar-group-caret">
                    <Icon name="chevron-left" size={11} />
                  </span>
                  {title} <span className="sidebar-count">{g.hosts.length}</span>
                </div>
                {!isCollapsed && <div className="sidebar-list">{g.hosts.map(renderHost)}</div>}
              </div>
            )
          })
        )}
      </div>

      <div className="sidebar-foot">
        <span className="meta">RUST · SSH2 · SFTP</span>
      </div>
    </aside>
  )
}
