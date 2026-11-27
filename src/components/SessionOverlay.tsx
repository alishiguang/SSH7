/**
 * 会话状态浮层。
 *
 * 终端**一直留在下面** —— 连接断了不该把用户屏幕上的内容抹掉，
 * 浮层只是盖在上面，把「发生了什么、现在在做什么」说清楚。
 *
 * 三种形态：
 *
 * - **正在连接** —— 第一次连上这台机器；
 * - **正在重新连接** —— 断线后自己接回去，带动画；
 * - **连接已断开 / 连接失败** —— 给出原因和「重新连接」按钮。
 *
 * 文案只说人话：不说「ECONNRESET」，也不说「socket closed」。
 */

import type { SessionState } from '../store/sessions'

import { Icon } from './Icons'

interface Props {
  session: SessionState
  /** 重新连接 */
  onReconnect: () => void
  /** 关掉这个标签 */
  onClose: () => void
}

/**
 * 重连中的信号弧。
 *
 * 一圈慢慢转的弧 —— 和「信号」是一回事，比转圈loader 更贴 SSH 的语境。
 */
function SignalArc() {
  return (
    <svg className="signal-arc" viewBox="0 0 48 48" width="46" height="46" aria-hidden>
      <circle className="signal-arc-track" cx="24" cy="24" r="19" />
      <circle className="signal-arc-live" cx="24" cy="24" r="19" />
    </svg>
  )
}

/** 断线的信号：信号波纹被划掉。 */
function SignalOff() {
  return (
    <svg className="signal-off" viewBox="0 0 48 48" width="46" height="46" aria-hidden>
      <path d="M6 30A18 18 0 0 1 42 30" />
      <path d="M14 37A11 11 0 0 1 34 37" />
      <circle cx="24" cy="42" r="2" />
      <path d="M11 9 37 39" />
    </svg>
  )
}

export default function SessionOverlay({ session, onReconnect, onClose }: Props) {
  const { status, label, host, username, error, attempt } = session
  const target = `${username}@${host}`

  /* ── 第一次连上这台机器 ───────────────────────── */
  if (status === 'connecting') {
    return (
      <div className="session-overlay">
        <div className="session-panel is-connecting">
          <span className="spinner" />
          <div className="session-panel-title">正在连接 {label}</div>
          <div className="session-panel-sub">{target}</div>
        </div>
      </div>
    )
  }

  /* ── 正在接回去 ───────────────────────────────── */
  if (status === 'reconnecting') {
    return (
      <div className="session-overlay">
        <div className="session-panel is-reconnecting">
          <SignalArc />
          <div className="session-panel-title">正在重新连接</div>
          <div className="session-panel-sub">{target}</div>
          {attempt && attempt > 1 ? (
            <div className="session-panel-note">第 {attempt} 次尝试</div>
          ) : null}
          <div className="reconnect-dots" aria-hidden>
            <i />
            <i />
            <i />
          </div>
        </div>
      </div>
    )
  }

  /* ── 断了 / 失败 ───────────────────────────────── */
  const failed = status === 'failed'
  return (
    <div className="session-overlay">
      <div className={`session-panel is-${failed ? 'failed' : 'lost'}`}>
        {failed ? <Icon name="alert" size={24} /> : <SignalOff />}
        <div className="session-panel-title">
          {failed ? '连接失败' : '连接已断开'}
        </div>
        <div className="session-panel-sub">{target}</div>
        {error ? (
          <>
            <div className="session-panel-msg">{error.message}</div>
            {error.hint ? (
              <div className="session-panel-hint">{error.hint}</div>
            ) : null}
          </>
        ) : null}
        <div className="session-panel-actions">
          <button className="btn btn-primary btn-sm" onClick={onReconnect}>
            <Icon name="refresh" size={12} />
            重新连接
          </button>
          <button className="btn btn-ghost btn-sm" onClick={onClose}>
            关闭标签
          </button>
        </div>
      </div>
    </div>
  )
}
