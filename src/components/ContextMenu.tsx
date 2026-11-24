import { useEffect, useLayoutEffect, useRef, useState } from 'react'

import { Icon, type IconName } from './Icons'

export interface ContextMenuItem {
  id: string
  label: string
  icon?: IconName
  danger?: boolean
  onSelect: () => void
}

interface Props {
  x: number
  y: number
  items: ContextMenuItem[]
  onClose: () => void
}

/**
 * 轻量右键菜单。
 *
 * 位置会自动收敛到视口内，避免菜单被窗口边缘裁掉。
 */
export default function ContextMenu({ x, y, items, onClose }: Props) {
  const ref = useRef<HTMLDivElement>(null)
  const [pos, setPos] = useState({ x, y })

  // 贴近视口边缘时向内翻转。
  useLayoutEffect(() => {
    const el = ref.current
    if (!el) return
    const rect = el.getBoundingClientRect()
    const pad = 8
    let nx = x
    let ny = y
    if (x + rect.width + pad > window.innerWidth) {
      nx = Math.max(pad, window.innerWidth - rect.width - pad)
    }
    if (y + rect.height + pad > window.innerHeight) {
      ny = Math.max(pad, window.innerHeight - rect.height - pad)
    }
    setPos({ x: nx, y: ny })
  }, [x, y])

  // 点击别处或按 Esc 关闭。
  useEffect(() => {
    const onPointerDown = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) onClose()
    }
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose()
    }
    window.addEventListener('mousedown', onPointerDown)
    window.addEventListener('keydown', onKey)
    return () => {
      window.removeEventListener('mousedown', onPointerDown)
      window.removeEventListener('keydown', onKey)
    }
  }, [onClose])

  return (
    <div
      className="context-menu"
      ref={ref}
      style={{ left: pos.x, top: pos.y }}
      role="menu"
      onContextMenu={(e) => e.preventDefault()}
    >
      {items.map((item) => (
        <button
          key={item.id}
          role="menuitem"
          className={`context-menu-item${item.danger ? ' is-danger' : ''}`}
          onClick={() => {
            item.onSelect()
            onClose()
          }}
        >
          {item.icon && <Icon name={item.icon} size={12} />}
          <span>{item.label}</span>
        </button>
      ))}
    </div>
  )
}
