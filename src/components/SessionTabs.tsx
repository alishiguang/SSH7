import { useSessions } from '../store/sessions'
import { Icon } from './Icons'

interface Props {
  /** 会话右键菜单。 */
  onSessionMenu: (sessionId: string, x: number, y: number) => void
}

/** 顶部会话标签栏。 */
export default function SessionTabs({ onSessionMenu }: Props) {
  const sessions = useSessions((s) => s.sessions)
  const activeId = useSessions((s) => s.activeId)
  const setActive = useSessions((s) => s.setActive)
  const removeSession = useSessions((s) => s.remove)

  if (sessions.length === 0) return null

  return (
    <div className="tabs" role="tablist">
      {sessions.map((s) => (
        <div
          key={s.id}
          role="tab"
          aria-selected={activeId === s.id}
          className={`tab${activeId === s.id ? ' is-active' : ''}`}
          onClick={() => setActive(s.id)}
          onContextMenu={(e) => {
            e.preventDefault()
            onSessionMenu(s.id, e.clientX, e.clientY)
          }}
          title={`${s.label} — 右键查看更多操作`}
        >
          <span className={`status-dot status-${s.status}`} />
          <span className="tab-label">{s.label}</span>
          {s.cwd && <span className="tab-cwd">{s.cwd}</span>}
          {s.status === 'failed' && (
            <Icon name="alert" size={11} className="tab-warn" />
          )}
          <button
            className="tab-close"
            title="关闭会话"
            onClick={(e) => {
              e.stopPropagation()
              removeSession(s.id)
            }}
          >
            <Icon name="x" size={11} />
          </button>
        </div>
      ))}
    </div>
  )
}
