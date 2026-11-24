import { useEffect, useRef, useState } from 'react'

import { lockDragCursor } from '../lib/drag'
import { useSessions } from '../store/sessions'
import { Icon } from './Icons'
import type { ConfirmSpec } from './ConfirmDialog'

interface Props {
  /** 会话右键菜单。 */
  onSessionMenu: (sessionId: string, x: number, y: number) => void
  /** 开始改名（双击标签或右键菜单）。 */
  onRenameStart: (sessionId: string) => void
  /** 正在改名的会话 id（null 表示没有）。 */
  renamingId: string | null
  /** 改名结束：`label` 为 null 表示取消。 */
  onRenameEnd: (sessionId: string, label: string | null) => void
  /** 关闭前的二次确认。 */
  askConfirm: (spec: ConfirmSpec) => void
}

/** 拖动超过这个距离（px）才算拖动，否则当成普通点击。 */
const DRAG_THRESHOLD = 6

export default function SessionTabs({
  onSessionMenu,
  onRenameStart,
  renamingId,
  onRenameEnd,
  askConfirm,
}: Props) {
  const sessions = useSessions((s) => s.sessions)
  const activeId = useSessions((s) => s.activeId)
  const setActive = useSessions((s) => s.setActive)
  const removeSession = useSessions((s) => s.remove)
  const moveSession = useSessions((s) => s.move)

  const [draft, setDraft] = useState('')
  const inputRef = useRef<HTMLInputElement>(null)
  const barRef = useRef<HTMLDivElement>(null)

  /* ── 拖动排序 ─────────────────────────────────── */
  const [dragId, setDragId] = useState<string | null>(null)
  const [dropIndex, setDropIndex] = useState<number | null>(null)
  /** 拖动过程中的实时状态，放 ref 里避免每帧重渲染。 */
  const dragRef = useRef<{ id: string; startX: number; started: boolean } | null>(null)
  const dropIndexRef = useRef<number | null>(null)
  const skipClickRef = useRef(false)

  /**
   * 指针落在哪儿 —— 返回**去掉被拖标签之后**的插入序号。
   *
   * 这个序号直接就是被拖标签挪动之后的最终下标，
   * 和 store 里 `move(id, toIndex)` 的语义一致，不用再换算。
   */
  const dropIndexAt = (clientX: number, draggingId: string): number => {
    const bar = barRef.current
    if (!bar) return 0
    const tabs = Array.from(bar.querySelectorAll<HTMLElement>('.tab'))
    let idx = 0
    for (const tab of tabs) {
      if (tab.dataset.id === draggingId) continue
      const r = tab.getBoundingClientRect()
      if (clientX < r.left + r.width / 2) break
      idx++
    }
    return idx
  }

  const startDrag = (e: React.PointerEvent, id: string) => {
    if (e.button !== 0) return
    // 点在关闭按钮上不算拖动
    if ((e.target as HTMLElement).closest('.tab-close')) return
    dragRef.current = { id, startX: e.clientX, started: false }

    const onMove = (ev: PointerEvent) => {
      const d = dragRef.current
      if (!d) return
      if (!d.started) {
        if (Math.abs(ev.clientX - d.startX) < DRAG_THRESHOLD) return
        d.started = true
        setDragId(d.id)
        lockDragCursor(true)
      }
      const idx = dropIndexAt(ev.clientX, d.id)
      dropIndexRef.current = idx
      setDropIndex(idx)
    }

    const onUp = () => {
      const d = dragRef.current
      dragRef.current = null
      window.removeEventListener('pointermove', onMove)
      window.removeEventListener('pointerup', onUp)
      const idx = dropIndexRef.current
      dropIndexRef.current = null
      setDropIndex(null)
      setDragId(null)
      lockDragCursor(false)
      if (!d?.started) return
      skipClickRef.current = true
      window.setTimeout(() => {
        skipClickRef.current = false
      }, 0)
      if (idx != null) moveSession(d.id, idx)
    }

    window.addEventListener('pointermove', onMove)
    window.addEventListener('pointerup', onUp)
  }

  /* ── 改名 ─────────────────────────────────────── */
  useEffect(() => {
    if (!renamingId) return
    // 从 store 里直接读，别把 sessions 放进依赖 —— 否则 cwd 一变就把正在打的字冲掉。
    const s = useSessions.getState().sessions.find((x) => x.id === renamingId)
    setDraft(s?.label ?? '')
    // 输入框这一轮渲染完才会挂上，聚焦要等 DOM 更新完
    const t = window.setTimeout(() => {
      inputRef.current?.focus()
      inputRef.current?.select()
    }, 0)
    return () => window.clearTimeout(t)
  }, [renamingId])

  if (sessions.length === 0) return null

  /* 落点红线：插进去之后，前面是谁、后面是谁。 */
  const others = dragId ? sessions.filter((s) => s.id !== dragId) : []
  const afterId = dropIndex != null ? others[dropIndex]?.id : undefined
  const beforeId = dropIndex != null && dropIndex > 0 ? others[dropIndex - 1]?.id : undefined

  return (
    <div className="tabs" role="tablist" ref={barRef}>
      {sessions.map((s) => {
        const isRenaming = renamingId === s.id
        const dropBefore = s.id === afterId
        const dropAfter = s.id === beforeId
        return (
          <div
            key={s.id}
            data-id={s.id}
            role="tab"
            aria-selected={activeId === s.id}
            className={[
              'tab',
              activeId === s.id ? 'is-active' : '',
              dragId === s.id ? 'is-dragging' : '',
              dropBefore ? 'is-drop-before' : '',
              dropAfter ? 'is-drop-after' : '',
            ]
              .filter(Boolean)
              .join(' ')}
            onClick={() => {
              if (skipClickRef.current) return
              setActive(s.id)
            }}
            onDoubleClick={() => onRenameStart(s.id)}
            onPointerDown={(e) => startDrag(e, s.id)}
            onContextMenu={(e) => {
              e.preventDefault()
              onSessionMenu(s.id, e.clientX, e.clientY)
            }}
            title={`${s.label} — 双击改名，拖动可调整顺序`}
          >
            <span className={`status-dot status-${s.status}`} />
            {isRenaming ? (
              <input
                ref={inputRef}
                className="tab-rename"
                value={draft}
                onChange={(e) => setDraft(e.target.value)}
                onClick={(e) => e.stopPropagation()}
                onDoubleClick={(e) => e.stopPropagation()}
                onBlur={() => onRenameEnd(s.id, draft.trim())}
                onKeyDown={(e) => {
                  e.stopPropagation()
                  if (e.key === 'Enter') {
                    e.preventDefault()
                    onRenameEnd(s.id, draft.trim())
                  } else if (e.key === 'Escape') {
                    e.preventDefault()
                    onRenameEnd(s.id, null)
                  }
                }}
              />
            ) : (
              <span className="tab-label">{s.label}</span>
            )}
            {s.cwd && <span className="tab-cwd">{s.cwd}</span>}
            {s.status === 'failed' && (
              <Icon name="alert" size={11} className="tab-warn" />
            )}
            <button
              className="tab-close"
              title="关闭会话"
              onClick={(e) => {
                e.stopPropagation()
                // 关闭会话 = 断连，先问一句 —— 远端正在跑的东西说没就没了。
                askConfirm({
                  title: 'CLOSE SESSION',
                  message: (
                    <>
                      关闭会话「<span className="confirm-target">{s.label}</span>」？
                      <span className="confirm-hint">
                        连接会断开，远端正在运行的进程可能受影响。
                      </span>
                    </>
                  ),
                  confirmLabel: '关闭会话',
                  danger: true,
                  onConfirm: () => removeSession(s.id),
                })
              }}
            >
              <Icon name="x" size={11} />
            </button>
          </div>
        )
      })}
    </div>
  )
}
