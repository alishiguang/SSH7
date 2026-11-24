import { useEffect, useRef } from 'react'

import { useUi } from '../store/ui'

import { Icon } from './Icons'

export interface ConfirmSpec {
  /** 标题栏上的那行字，通常是动作名。 */
  title: string
  /** 说清楚「删什么」和「会丢什么」。 */
  message: React.ReactNode
  /** 确认按钮上的字。 */
  confirmLabel?: string
  /** 危险操作 —— 确认按钮用红色。 */
  danger?: boolean
  onConfirm: () => void
}

interface Props {
  spec: ConfirmSpec | null
  onClose: () => void
}

/**
 * 删除前的二次确认。
 *
 * 凡是会丢东西的操作都走这里 —— 主机、分组、会话、远端文件、设置。
 * 一次误点不该换来配置消失，所以确认框由应用自己画，
 * 不用浏览器那个 `window.confirm`（样式突兀，还会被系统拦在前面）。
 */
export default function ConfirmDialog({ spec, onClose }: Props) {
  const setConfirmOpen = useUi((s) => s.setConfirmOpen)
  const confirmRef = useRef<HTMLButtonElement>(null)

  useEffect(() => {
    if (!spec) return
    setConfirmOpen(true)
    // 焦点落在确认键上 —— 按回车就是确认，符合直觉。
    const t = window.setTimeout(() => confirmRef.current?.focus(), 0)
    return () => {
      window.clearTimeout(t)
      setConfirmOpen(false)
    }
  }, [spec, setConfirmOpen])

  // Esc = 取消。
  useEffect(() => {
    if (!spec) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault()
        e.stopPropagation()
        onClose()
      }
    }
    window.addEventListener('keydown', onKey, true)
    return () => window.removeEventListener('keydown', onKey, true)
  }, [spec, onClose])

  if (!spec) return null

  const handleConfirm = () => {
    spec.onConfirm()
    onClose()
  }

  return (
    <div
      className="modal-backdrop confirm-backdrop"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose()
      }}
    >
      <div className="confirm" role="alertdialog" aria-modal="true" aria-label={spec.title}>
        <div className="confirm-head">
          <span className="meta">{spec.title}</span>
          <button className="icon-btn" onClick={onClose} aria-label="关闭">
            <Icon name="x" size={13} />
          </button>
        </div>

        <div className="confirm-body">{spec.message}</div>

        <div className="confirm-actions">
          <button className="btn btn-ghost" onClick={onClose}>
            取消
          </button>
          <button
            ref={confirmRef}
            className={`btn ${spec.danger ? 'btn-danger' : 'btn-primary'}`}
            onClick={handleConfirm}
          >
            {spec.confirmLabel ?? '确认'}
          </button>
        </div>
      </div>
    </div>
  )
}
