import { useEffect, useMemo, useRef, useState } from 'react'

import type { SavedHost } from '../api/types'
import { lockDragCursor } from '../lib/drag'
import { useHosts } from '../store/hosts'
import { useUi } from '../store/ui'
import { Icon } from './Icons'
import type { ConfirmSpec } from './ConfirmDialog'

interface Props {
  onConnect: () => void
  /** 开启新会话（悬停时的 ＋ 与右键菜单里的「开启新会话」）。 */
  onOpenNewSession: (host: SavedHost) => void
  /** 双击主机行：把这台机器开到一个独立窗口里。 */
  onOpenInWindow: (host: SavedHost) => void
  onEditHost: (host: SavedHost) => void
  /** 主机右键菜单。 */
  onHostMenu: (host: SavedHost, x: number, y: number) => void
  /** 分组右键菜单。 */
  onGroupMenu: (group: string, x: number, y: number) => void
  /** 删除前的二次确认。 */
  askConfirm: (spec: ConfirmSpec) => void
}

/** 一个分组。`name` 为空表示「未分组」。 */
interface HostGroup {
  name: string
  hosts: SavedHost[]
}

/** 侧边栏里正在编辑分组的方式。 */
type GroupEdit = { mode: 'create' } | { mode: 'rename'; name: string } | null

/** 拖动超过这个距离（px）才算拖动，否则当成普通点击。 */
const DRAG_THRESHOLD = 5

export default function Sidebar({
  onConnect,
  onOpenNewSession,
  onOpenInWindow,
  onEditHost,
  onHostMenu,
  onGroupMenu,
  askConfirm,
}: Props) {
  const collapsed = useUi((s) => s.sidebarCollapsed)
  const toggleSidebar = useUi((s) => s.toggleSidebar)
  const filePanelOpen = useUi((s) => s.filePanelOpen)
  const setFilePanel = useUi((s) => s.setFilePanel)
  const push = useUi((s) => s.push)
  const reportError = useUi((s) => s.reportError)
  const selectedHostId = useUi((s) => s.selectedHostId)
  const setSelectedHost = useUi((s) => s.setSelectedHost)

  const hosts = useHosts((s) => s.hosts)
  const registry = useHosts((s) => s.groups)
  const removeHost = useHosts((s) => s.remove)
  const createGroup = useHosts((s) => s.createGroup)
  const renameGroup = useHosts((s) => s.renameGroup)
  const removeGroup = useHosts((s) => s.removeGroup)
  const reorderGroups = useHosts((s) => s.reorderGroups)
  const moveHostToGroup = useHosts((s) => s.moveHostToGroup)

  const [hoverHost, setHoverHost] = useState<string | null>(null)
  const [hoverGroup, setHoverGroup] = useState<string | null>(null)
  /** 折叠起来的分组名。 */
  const [collapsedGroups, setCollapsedGroups] = useState<string[]>([])
  /** 正在新建 / 改名的分组。 */
  const [groupEdit, setGroupEdit] = useState<GroupEdit>(null)
  const [groupDraft, setGroupDraft] = useState('')
  const groupInputRef = useRef<HTMLInputElement>(null)
  /** 本轮编辑是否已经提交过（防止 blur 与回车重复提交）。 */
  const committingRef = useRef(false)

  /* ── 拖动：主机换组 / 分组排序 ───────────────────── */
  /** 正在拖动的主机。 */
  const [dragHostId, setDragHostId] = useState<string | null>(null)
  /** 主机拖动时悬停到的目标分组（空串 = 未分组）。 */
  const [dropGroupName, setDropGroupName] = useState<string | null>(null)
  const dropGroupRef = useRef<string | null>(null)
  /** 正在拖动的分组。 */
  const [dragGroupName, setDragGroupName] = useState<string | null>(null)
  /** 分组排序的落点（去掉被拖的那个之后，插在第几位）。 */
  const [groupDropIndex, setGroupDropIndex] = useState<number | null>(null)
  const groupDropIndexRef = useRef<number | null>(null)
  /** 拖完之后的这一小段时间里，别把落点当成点击。 */
  const skipClickRef = useRef(false)
  /** 各分组标题的 DOM，用来算落点。 */
  const groupHeadRefs = useRef(new Map<string, HTMLElement>())

  /**
   * 主机按分组归拢。
   *
   * 分组清单（`groups`）是权威：它决定顺序，也允许**空分组**存在 ——
   * 用户可以先把「生产 / 测试」建好，往里放机器时直接选。
   * 清单里没有、但主机上写着的分组（比如老版本留下的）照样显示，
   * 追加在清单后面。
   */
  const groups = useMemo<HostGroup[]>(() => {
    const map = new Map<string, SavedHost[]>()
    for (const name of registry) map.set(name, [])
    for (const h of hosts) {
      const key = (h.group ?? '').trim()
      const bucket = map.get(key)
      if (bucket) bucket.push(h)
      else map.set(key, [h])
    }
    const entries = [...map.entries()]
    // 没分组的排最前，其余按清单顺序（清单外新出现的接在后面）
    const ungrouped = entries.filter(([name]) => name === '')
    const rest = entries.filter(([name]) => name !== '')
    return [...ungrouped, ...rest].map(([name, list]) => ({ name, hosts: list }))
  }, [hosts, registry])

  /** 具名的分组（未分组固定在最前，不参与排序）。 */
  const namedGroups = useMemo(() => groups.filter((g) => g.name !== ''), [groups])

  const toggleGroup = (name: string) =>
    setCollapsedGroups((prev) =>
      prev.includes(name) ? prev.filter((g) => g !== name) : [...prev, name],
    )

  /* ── 拖主机：放到别的分组 ───────────────────────── */
  /** 指针下面是不是某个分组标题？返回分组名，空串表示「未分组」。 */
  const groupAtPoint = (x: number, y: number): string | null => {
    const el = document.elementFromPoint(x, y)
    const head = el?.closest('.sidebar-group-head') as HTMLElement | null
    if (!head) return null
    return head.dataset.group ?? null
  }

  const startHostDrag = (e: React.PointerEvent, host: SavedHost) => {
    if (e.button !== 0) return
    // 点在按钮上（＋ / 编辑 / ×）不算拖动
    if ((e.target as HTMLElement).closest('button')) return

    const startY = e.clientY
    let started = false

    const onMove = (ev: PointerEvent) => {
      if (!started) {
        if (Math.abs(ev.clientY - startY) < DRAG_THRESHOLD) return
        started = true
        setDragHostId(host.id)
        lockDragCursor(true)
      }
      const target = groupAtPoint(ev.clientX, ev.clientY)
      dropGroupRef.current = target
      setDropGroupName(target)
    }

    const onUp = () => {
      window.removeEventListener('pointermove', onMove)
      window.removeEventListener('pointerup', onUp)
      const target = dropGroupRef.current
      dropGroupRef.current = null
      setDragHostId(null)
      setDropGroupName(null)
      lockDragCursor(false)
      if (!started) return
      // 落在分组标题之外就当没拖
      if (target === null) return
      skipClickRef.current = true
      window.setTimeout(() => {
        skipClickRef.current = false
      }, 0)
      const current = (host.group ?? '').trim()
      if (target === current) return
      void moveHostToGroup(host.id, target || undefined)
        .then(() => {
          push({
            kind: 'info',
            title: '已移动',
            message: target ? `${host.label} → ${target}` : `${host.label} → 未分组`,
          })
        })
        .catch((e) => reportError('移动主机失败', e))
    }

    window.addEventListener('pointermove', onMove)
    window.addEventListener('pointerup', onUp)
  }

  /* ── 拖分组：调整顺序 ───────────────────────────── */
  /** 指针落在第几个分组之前（去掉正在拖动的那个）。 */
  const groupIndexAt = (clientY: number, draggingName: string): number => {
    const others = namedGroups.filter((g) => g.name !== draggingName)
    let idx = 0
    for (const g of others) {
      const el = groupHeadRefs.current.get(g.name)
      if (!el) continue
      const r = el.getBoundingClientRect()
      if (clientY < r.top + r.height / 2) break
      idx++
    }
    return idx
  }

  const startGroupDrag = (e: React.PointerEvent, name: string) => {
    if (e.button !== 0) return
    if ((e.target as HTMLElement).closest('button, input')) return

    const startY = e.clientY
    let started = false

    const onMove = (ev: PointerEvent) => {
      if (!started) {
        if (Math.abs(ev.clientY - startY) < DRAG_THRESHOLD) return
        started = true
        setDragGroupName(name)
        lockDragCursor(true)
      }
      const idx = groupIndexAt(ev.clientY, name)
      groupDropIndexRef.current = idx
      setGroupDropIndex(idx)
    }

    const onUp = () => {
      window.removeEventListener('pointermove', onMove)
      window.removeEventListener('pointerup', onUp)
      const idx = groupDropIndexRef.current
      groupDropIndexRef.current = null
      setGroupDropIndex(null)
      setDragGroupName(null)
      lockDragCursor(false)
      if (!started || idx === null) return
      skipClickRef.current = true
      window.setTimeout(() => {
        skipClickRef.current = false
      }, 0)

      // 新顺序：去掉自己，再插到落点
      const others = namedGroups.filter((g) => g.name !== name)
      others.splice(idx, 0, { name, hosts: [] })
      void reorderGroups(others.map((g) => g.name)).catch((e) => reportError('调整分组顺序失败', e))
    }

    window.addEventListener('pointermove', onMove)
    window.addEventListener('pointerup', onUp)
  }

  /* ── 分组：新建 / 改名 / 删除 ───────────────────── */
  useEffect(() => {
    if (!groupEdit) return
    const t = window.setTimeout(() => {
      groupInputRef.current?.focus()
      groupInputRef.current?.select()
    }, 0)
    return () => window.clearTimeout(t)
  }, [groupEdit])

  const startCreateGroup = () => {
    committingRef.current = false
    setGroupDraft('')
    setGroupEdit({ mode: 'create' })
  }

  const startRenameGroup = (name: string) => {
    committingRef.current = false
    setGroupDraft(name)
    setGroupEdit({ mode: 'rename', name })
  }

  const commitGroupEdit = () => {
    // 回车提交后输入框会被卸载，可能再触发一次 blur —— 只提交一次。
    if (committingRef.current) return
    committingRef.current = true
    const edit = groupEdit
    const next = groupDraft.trim()
    setGroupEdit(null)
    if (!edit || !next) return

    if (edit.mode === 'create') {
      if (registry.includes(next)) {
        push({ kind: 'info', title: '分组已存在', message: `「${next}」已经在列表里了` })
        return
      }
      void createGroup(next)
        .then(() => {
          push({ kind: 'success', title: '已新建分组', message: next })
        })
        .catch((e) => reportError('新建分组失败', e))
      return
    }

    const old = edit.name
    if (next === old) return
    void renameGroup(old, next)
      .then(() => {
        // 折叠状态跟着改名走，不然组一改名就自动展开了。
        setCollapsedGroups((prev) => prev.map((g) => (g === old ? next : g)))
        push({
          kind: 'success',
          title: '分组已改名',
          message: `「${old}」→「${next}」`,
        })
      })
      .catch((e) => reportError('分组改名失败', e))
  }

  /** 删除分组 —— 先确认，免得手一抖整组就没了。 */
  const deleteGroup = (g: HostGroup) => {
    askConfirm({
      title: 'DELETE GROUP',
      message: (
        <>
          删除分组「<span className="confirm-target">{g.name}</span>」？
          <span className="confirm-hint">
            组里的 {g.hosts.length} 台机器会回到「未分组」，机器本身不会被删。
          </span>
        </>
      ),
      confirmLabel: '删除分组',
      danger: true,
      onConfirm: () => {
        void removeGroup(g.name)
          .then(() => {
            setCollapsedGroups((prev) => prev.filter((x) => x !== g.name))
            push({
              kind: 'info',
              title: '已删除分组',
              message: `「${g.name}」的 ${g.hosts.length} 台主机回到未分组`,
            })
          })
          .catch((e) => reportError('删除分组失败', e))
      },
    })
  }

  /** 删除主机 —— 同样先确认。 */
  const deleteHost = (h: SavedHost) => {
    askConfirm({
      title: 'DELETE HOST',
      message: (
        <>
          删除主机「<span className="confirm-target">{h.label}</span>」？
          <span className="confirm-hint">
            地址、用户名、凭据都会一起删掉，无法恢复。
          </span>
        </>
      ),
      confirmLabel: '删除',
      danger: true,
      onConfirm: () => void removeHost(h.id),
    })
  }

  if (collapsed) {
    return (
      <aside className="sidebar sidebar-collapsed">
        <button className="icon-btn" onClick={toggleSidebar} title="展开侧边栏">
          <Icon name="menu" size={15} />
        </button>
        <button className="icon-btn icon-btn-accent" onClick={onConnect} title="新建连接">
          <Icon name="plus" size={15} />
        </button>
      </aside>
    )
  }

  const renderHost = (h: SavedHost) => (
    <div
      key={h.id}
      className={[
        'host-row',
        selectedHostId === h.id ? 'is-picked' : '',
        dragHostId === h.id ? 'is-dragging' : '',
      ]
        .filter(Boolean)
        .join(' ')}
      onClick={() => {
        if (skipClickRef.current) return
        // 单击只是选中 —— 建连接要走 ＋ 或右键菜单。
        // 再点一次同一台就是取消选中。
        setSelectedHost(selectedHostId === h.id ? null : h.id)
      }}
      onDoubleClick={() => {
        if (skipClickRef.current) return
        // 双击 = 把这台机器开到一个独立窗口里。
        // 前两次单击已经把「选中 → 取消选中」走了一遍，这里把状态摆正。
        setSelectedHost(h.id)
        onOpenInWindow(h)
      }}
      onContextMenu={(e) => {
        e.preventDefault()
        onHostMenu(h, e.clientX, e.clientY)
      }}
      onMouseEnter={() => setHoverHost(h.id)}
      onMouseLeave={() => setHoverHost(null)}
      onPointerDown={(e) => startHostDrag(e, h)}
      title="单击选中；双击在新窗口打开；悬停出现 ＋ 开启新会话；右键有更多操作；可拖到别的分组"
    >
      <Icon name="server" size={13} className="host-icon" />
      <div className="host-row-body">
        <div className="host-row-label">{h.label}</div>
        <div className="host-row-meta">
          {h.username}@{h.host}:{h.port}
        </div>
      </div>
      {hoverHost === h.id && (
        <div className="host-row-actions">
          <button
            className="icon-btn icon-btn-accent"
            title="开启新会话"
            onClick={(e) => {
              e.stopPropagation()
              onOpenNewSession(h)
            }}
          >
            <Icon name="plus" size={12} />
          </button>
          <button
            className="icon-btn"
            title="编辑"
            onClick={(e) => {
              e.stopPropagation()
              onEditHost(h)
            }}
          >
            <Icon name="edit" size={12} />
          </button>
          <button
            className="icon-btn icon-btn-danger"
            title="删除"
            onClick={(e) => {
              e.stopPropagation()
              deleteHost(h)
            }}
          >
            <Icon name="x" size={12} />
          </button>
        </div>
      )}
    </div>
  )

  return (
    <aside className="sidebar">
      <div className="sidebar-head">
        <span className="brand-mark display">SSH7</span>
        <button className="icon-btn" onClick={toggleSidebar} title="收起侧边栏">
          <Icon name="chevron-left" size={14} />
        </button>
      </div>

      <div className="sidebar-tools">
        <button className="tool-btn" onClick={startCreateGroup} title="新建一个分组">
          <Icon name="plus" size={12} />
          <span>分组</span>
        </button>
        <button className="tool-btn tool-btn-primary" onClick={onConnect}>
          <Icon name="plus" size={12} />
          <span>连接</span>
        </button>
        <button
          className={`tool-btn${filePanelOpen ? ' is-on' : ''}`}
          onClick={() => setFilePanel(!filePanelOpen)}
          title="切换文件面板"
        >
          <Icon name="folder" size={12} />
          <span>文件</span>
        </button>
      </div>

      <div className="sidebar-section sidebar-section-grow">
        {groups.length === 0 && groupEdit?.mode !== 'create' && (
          <div className="sidebar-empty">
            <Icon name="server" size={18} />
            <span>还没有保存的主机</span>
          </div>
        )}
        {groups.map((g) => {
            const isCollapsed = collapsedGroups.includes(g.name)
            const isRenaming = groupEdit?.mode === 'rename' && groupEdit.name === g.name
            const title = g.name || 'HOSTS'
            const isDragging = dragGroupName === g.name
            // 落点红线：插进去之后，前后各是谁
            const others = namedGroups.filter((x) => x.name !== dragGroupName)
            const dropBefore =
              dragGroupName !== null && groupDropIndex !== null
                ? others[groupDropIndex]?.name === g.name
                : false
            const dropAfter =
              dragGroupName !== null && groupDropIndex !== null && groupDropIndex > 0
                ? others[groupDropIndex - 1]?.name === g.name
                : false
            return (
              <div className="sidebar-group" key={g.name || '__none'}>
                <div
                  ref={(el) => {
                    if (el) groupHeadRefs.current.set(g.name, el)
                    else groupHeadRefs.current.delete(g.name)
                  }}
                  data-group={g.name}
                  className={[
                    'meta',
                    'sidebar-section-head',
                    'sidebar-group-head',
                    isCollapsed ? 'is-collapsed' : '',
                    isDragging ? 'is-dragging' : '',
                    dropBefore ? 'is-drop-before' : '',
                    dropAfter ? 'is-drop-after' : '',
                    dropGroupName === g.name ? 'is-drop-target' : '',
                  ]
                    .filter(Boolean)
                    .join(' ')}
                  onClick={() => {
                    if (skipClickRef.current) return
                    if (!isRenaming) toggleGroup(g.name)
                  }}
                  onMouseEnter={() => setHoverGroup(g.name)}
                  onMouseLeave={() => setHoverGroup(null)}
                  onContextMenu={(e) => {
                    // 未分组那一组没有「新连接落在哪个组」的概念 —— 不给菜单。
                    if (!g.name) return
                    e.preventDefault()
                    e.stopPropagation()
                    onGroupMenu(g.name, e.clientX, e.clientY)
                  }}
                  onPointerDown={(e) => {
                    if (g.name) startGroupDrag(e, g.name)
                  }}
                  title={g.name ? '折叠 / 展开分组；拖动可调整顺序；右键可新建连接' : undefined}
                >
                  <span className="sidebar-group-caret">
                    <Icon name="chevron-left" size={11} />
                  </span>
                  {isRenaming ? (
                    <input
                      ref={groupInputRef}
                      className="group-rename"
                      value={groupDraft}
                      onChange={(e) => setGroupDraft(e.target.value)}
                      onClick={(e) => e.stopPropagation()}
                      onDoubleClick={(e) => e.stopPropagation()}
                      onBlur={commitGroupEdit}
                      onKeyDown={(e) => {
                        e.stopPropagation()
                        if (e.key === 'Enter') {
                          e.preventDefault()
                          commitGroupEdit()
                        } else if (e.key === 'Escape') {
                          e.preventDefault()
                          // 取消：标记成已提交，免得随后那声 blur 又把它改回去。
                          committingRef.current = true
                          setGroupEdit(null)
                        }
                      }}
                    />
                  ) : (
                    <span className="sidebar-group-title">{title}</span>
                  )}
                  <span className="sidebar-count">{g.hosts.length}</span>
                  {g.name && !isRenaming && hoverGroup === g.name && (
                    <span className="sidebar-group-actions">
                      <button
                        className="icon-btn"
                        title="重命名分组"
                        onClick={(e) => {
                          e.stopPropagation()
                          startRenameGroup(g.name)
                        }}
                      >
                        <Icon name="edit" size={11} />
                      </button>
                      <button
                        className="icon-btn icon-btn-danger"
                        title="删除分组（里面的主机回到未分组）"
                        onClick={(e) => {
                          e.stopPropagation()
                          deleteGroup(g)
                        }}
                      >
                        <Icon name="trash" size={11} />
                      </button>
                    </span>
                  )}
                </div>
                {!isCollapsed && <div className="sidebar-list">{g.hosts.map(renderHost)}</div>}
              </div>
            )
          })}

        {/* 新建分组：在列表最下面加一行，就地改名。 */}
        {groupEdit?.mode === 'create' && (
          <div className="sidebar-group">
            <div className="meta sidebar-section-head sidebar-group-new">
              <span className="sidebar-group-caret">
                <Icon name="chevron-left" size={11} />
              </span>
              <input
                ref={groupInputRef}
                className="group-rename"
                value={groupDraft}
                placeholder="分组名"
                onChange={(e) => setGroupDraft(e.target.value)}
                onClick={(e) => e.stopPropagation()}
                onDoubleClick={(e) => e.stopPropagation()}
                onBlur={commitGroupEdit}
                onKeyDown={(e) => {
                  e.stopPropagation()
                  if (e.key === 'Enter') {
                    e.preventDefault()
                    commitGroupEdit()
                  } else if (e.key === 'Escape') {
                    e.preventDefault()
                    // 取消：标记成已提交，免得随后那声 blur 又把它建出来。
                    committingRef.current = true
                    setGroupEdit(null)
                  }
                }}
              />
              <span className="sidebar-count">0</span>
            </div>
          </div>
        )}
      </div>

      <div className="sidebar-foot">
        <span className="meta">RUST · SSH2 · SFTP</span>
      </div>
    </aside>
  )
}
