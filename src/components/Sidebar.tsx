import { useState } from 'react'

import type { SavedHost } from '../api/types'
import { useHosts } from '../store/hosts'
import { useSessions } from '../store/sessions'
import { useUi } from '../store/ui'
import { Icon } from './Icons'

interface Props {
  onConnect: () => void
  onSelectHost: (host: SavedHost) => void
  /** 强制为该主机新开一个会话（即使已有会话）。 */
  onOpenNewSession: (host: SavedHost) => void
  onEditHost: (host: SavedHost) => void
  /** 会话右键菜单。 */
  onSessionMenu: (sessionId: string, x: number, y: number) => void
}

export default function Sidebar({
  onConnect,
  onSelectHost,
  onOpenNewSession,
  onEditHost,
  onSessionMenu,
}: Props) {
  const collapsed = useUi((s) => s.sidebarCollapsed)
  const toggleSidebar = useUi((s) => s.toggleSidebar)
  const filePanelOpen = useUi((s) => s.filePanelOpen)
  const setFilePanel = useUi((s) => s.setFilePanel)

  const hosts = useHosts((s) => s.hosts)
  const removeHost = useHosts((s) => s.remove)

  const sessions = useSessions((s) => s.sessions)
  const activeId = useSessions((s) => s.activeId)
  const setActive = useSessions((s) => s.setActive)
  const removeSession = useSessions((s) => s.remove)

  const [hoverHost, setHoverHost] = useState<string | null>(null)
  const [hoverSession, setHoverSession] = useState<string | null>(null)

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

      {sessions.length > 0 && (
        <div className="sidebar-section">
          <div className="meta sidebar-section-head">
            SESSIONS <span className="sidebar-count">{sessions.length}</span>
          </div>
          <div className="sidebar-list">
            {sessions.map((s) => (
              <div
                key={s.id}
                className={`session-row${activeId === s.id ? ' is-active' : ''}`}
                onClick={() => setActive(s.id)}
                onContextMenu={(e) => {
                  e.preventDefault()
                  onSessionMenu(s.id, e.clientX, e.clientY)
                }}
                onMouseEnter={() => setHoverSession(s.id)}
                onMouseLeave={() => setHoverSession(null)}
              >
                <span className={`status-dot status-${s.status}`} />
                <div className="session-row-body">
                  <div className="session-row-label">{s.label}</div>
                  {s.cwd && <div className="session-row-cwd">{s.cwd}</div>}
                </div>
                {hoverSession === s.id && (
                  <button
                    className="icon-btn icon-btn-danger"
                    title="关闭会话"
                    onClick={(e) => {
                      e.stopPropagation()
                      removeSession(s.id)
                    }}
                  >
                    <Icon name="x" size={12} />
                  </button>
                )}
              </div>
            ))}
          </div>
        </div>
      )}

      <div className="sidebar-section sidebar-section-grow">
        <div className="meta sidebar-section-head">
          HOSTS <span className="sidebar-count">{hosts.length}</span>
        </div>

        {hosts.length === 0 ? (
          <div className="sidebar-empty">
            <Icon name="server" size={18} />
            <span>还没有保存的主机</span>
          </div>
        ) : (
          <div className="sidebar-list">
            {hosts.map((h) => (
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
            ))}
          </div>
        )}
      </div>

      <div className="sidebar-foot">
        <span className="meta">RUST · SSH2 · SFTP</span>
      </div>
    </aside>
  )
}
