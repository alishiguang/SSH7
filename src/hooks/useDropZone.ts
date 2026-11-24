import { useEffect, useRef, useState } from 'react'

import { isPointInside, useDrop } from '../store/drop'

/**
 * 把原生拖放事件路由到具体区域。
 *
 * 返回 `isOver` 表示光标当前悬停在元素内；`paths` 是本次拖入的文件路径。
 */
export function useDropZone<T extends HTMLElement>() {
  const ref = useRef<T | null>(null)
  const [isOver, setIsOver] = useState(false)

  const dragging = useDrop((s) => s.dragging)
  const position = useDrop((s) => s.position)
  const paths = useDrop((s) => s.paths)

  useEffect(() => {
    if (!dragging) {
      setIsOver(false)
      return
    }
    setIsOver(isPointInside(ref.current, position))
  }, [dragging, position])

  return { ref, isOver, paths }
}
