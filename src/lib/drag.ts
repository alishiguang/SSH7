/**
 * 拖动期间的光标与选择行为。
 *
 * 拖动时把光标锁成「抓手」并禁止选中文字 —— 不然指针移出被拖元素
 * 光标就变回箭头，拖动的手感一下就散了。
 *
 * 额外兜一层：指针在窗口外松开、或者窗口失焦时，浏览器不一定派发
 * pointerup，光标就会一直卡在「抓」上。所以这里自己盯着这两件事，
 * 无论拖动那边有没有收尾，都保证光标能变回原样。
 */
let releaseSafety: (() => void) | null = null

export function lockDragCursor(on: boolean): void {
  const body = document.body

  if (on) {
    body.style.cursor = 'grabbing'
    body.style.userSelect = 'none'
    if (releaseSafety) return

    const release = () => {
      window.removeEventListener('blur', release)
      document.removeEventListener('pointercancel', release)
      releaseSafety = null
      lockDragCursor(false)
    }
    releaseSafety = release
    window.addEventListener('blur', release)
    document.addEventListener('pointercancel', release)
    return
  }

  body.style.cursor = ''
  body.style.userSelect = ''
  if (releaseSafety) {
    window.removeEventListener('blur', releaseSafety)
    document.removeEventListener('pointercancel', releaseSafety)
    releaseSafety = null
  }
}
