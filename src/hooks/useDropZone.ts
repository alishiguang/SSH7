import { useEffect, useRef, useState } from 'react'

import { isPointInside, useDrop } from '../store/drop'

/**
 * 把原生拖放事件路由到具体区域。
 *
 * - `isOver`：光标当前悬停在元素内（用于显示拖放提示层）
 * - `dropped`：一次**松手**事件，且落点在这个元素内 —— 上传只认它
 */
export function useDropZone<T extends HTMLElement>() {
  const ref = useRef<T | null>(null)
  const [isOver, setIsOver] = useState(false)
  const [dropped, setDropped] = useState<{ seq: number; paths: string[] } | null>(null)

  const dragging = useDrop((s) => s.dragging)
  const position = useDrop((s) => s.position)
  const drop = useDrop((s) => s.dropped)

  // 悬停：只影响提示层，不触发任何副作用。
  useEffect(() => {
    if (!dragging) {
      setIsOver(false)
      return
    }
    setIsOver(isPointInside(ref.current, position))
  }, [dragging, position])

  // 松手：落点命中本区域才算一次交给本区域的拖放。
  useEffect(() => {
    if (!drop) return
    if (isPointInside(ref.current, drop.position)) {
      setDropped({ seq: drop.seq, paths: drop.paths })
    }
  }, [drop])

  return { ref, isOver, dropped }
}
