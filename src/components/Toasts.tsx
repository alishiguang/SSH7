import { useUi } from '../store/ui'
import { Icon } from './Icons'

/** 右下角通知栈。 */
export default function Toasts() {
  const toasts = useUi((s) => s.toasts)
  const dismiss = useUi((s) => s.dismiss)

  if (toasts.length === 0) return null

  return (
    <div className="toast-stack" role="status" aria-live="polite">
      {toasts.map((t) => (
        <div className={`toast toast-${t.kind}`} key={t.id}>
          <span className="toast-icon">
            <Icon
              name={t.kind === 'error' ? 'alert' : t.kind === 'success' ? 'check' : 'info'}
              size={13}
            />
          </span>
          <div className="toast-body">
            <div className="toast-title">{t.title}</div>
            {t.message && <div className="toast-message">{t.message}</div>}
            {t.hint && <div className="toast-hint">{t.hint}</div>}
            {t.detail && <div className="toast-detail">{t.detail}</div>}
          </div>
          <button className="toast-close" onClick={() => dismiss(t.id)} aria-label="关闭">
            <Icon name="x" size={11} />
          </button>
        </div>
      ))}
    </div>
  )
}
