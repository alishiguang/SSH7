import { describe, expect, it } from 'vitest'

import { isPointInside } from './drop'

/** 造一个只有 getBoundingClientRect 的假元素。 */
function fakeRect(left: number, top: number, right: number, bottom: number) {
  return {
    getBoundingClientRect: () => ({ left, top, right, bottom }),
  } as unknown as HTMLElement
}

describe('isPointInside', () => {
  const el = fakeRect(100, 50, 400, 300)

  it('落点在内部分算命中', () => {
    expect(isPointInside(el, { x: 200, y: 100 })).toBe(true)
  })

  it('边界上的点也算命中', () => {
    expect(isPointInside(el, { x: 100, y: 50 })).toBe(true)
    expect(isPointInside(el, { x: 400, y: 300 })).toBe(true)
  })

  it('落点在外部不算命中', () => {
    expect(isPointInside(el, { x: 99, y: 100 })).toBe(false)
    expect(isPointInside(el, { x: 200, y: 301 })).toBe(false)
  })

  it('元素或坐标缺失时不算命中', () => {
    expect(isPointInside(null, { x: 200, y: 100 })).toBe(false)
    expect(isPointInside(el, null)).toBe(false)
  })
})
