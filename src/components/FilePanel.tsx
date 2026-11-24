import { useCallback, useEffect, useRef, useState } from 'react'

import { BridgeError, sftpApi } from '../api/bridge'
import type { RemoteFile } from '../api/types'
import { useDropZone } from '../hooks/useDropZone'
import { useSessions } from '../store/sessions'
import { useTransfers } from '../store/transfers'
import { useUi } from '../store/ui'
import { Icon } from './Icons'
import type { ConfirmSpec } from './ConfirmDialog'

interface Props {
  sessionId: string
  onClose: () => void
  /** 删除前的二次确认。 */
  askConfirm: (spec: ConfirmSpec) => void
}

/** 远端路径拼接。避免旧版 `.replace('//','/')` 只替换第一处的问题。 */
export function joinRemote(dir: string, name: string): string {
  const base = dir === '/' || dir === '' ? '' : dir.replace(/\/+$/, '')
  return `${base}/${name}`
}

/** 远端路径的父目录。 */
export function parentRemote(path: string): string {
  const parts = path.split('/').filter(Boolean)
  parts.pop()
  return parts.length === 0 ? '/' : `/${parts.join('/')}`
}

function formatSize(size: number): string {
  if (!Number.isFinite(size) || size < 0) return '—'
  if (size < 1024) return `${size}B`
  if (size < 1024 * 1024) return `${(size / 1024).toFixed(1)}KB`
  if (size < 1024 * 1024 * 1024) return `${(size / 1024 / 1024).toFixed(1)}MB`
  return `${(size / 1024 / 1024 / 1024).toFixed(2)}GB`
}

function formatTime(unixSeconds: number): string {
  if (!unixSeconds) return '—'
  const d = new Date(unixSeconds * 1000)
  const p = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`
}

export default function FilePanel({ sessionId, onClose, askConfirm }: Props) {
  const [cwd, setCwd] = useState('/')
  const [pathInput, setPathInput] = useState('/')
  const [files, setFiles] = useState<RemoteFile[]>([])
  const [loading, setLoading] = useState(false)
  const [selected, setSelected] = useState<string | null>(null)
  const [error, setError] = useState<{ message: string; detail?: string } | null>(null)

  const reportError = useUi((s) => s.reportError)
  const push = useUi((s) => s.push)
  const transfers = useTransfers((s) => s.transfers)
  const startTransfer = useTransfers((s) => s.start)
  const finishTransfer = useTransfers((s) => s.finish)

  /**
   * 这个会话的远端工作目录 —— 由 OSC 7 跟踪得到。
   *
   * 文件面板跟着它走：打开就落在会话目录，终端里 `cd` 了也跟着换。
   * 用户自己浏览到别处不会被拽回来 —— 只有会话目录**变了**才跟随。
   */
  const sessionCwd = useSessions((s) => s.sessions.find((x) => x.id === sessionId)?.cwd)
  /** 浏览位置与会话目录不一致时，路径栏上给一个「跳回去」的入口。 */
  const offSessionDir = !!sessionCwd && cwd !== sessionCwd

  const { ref: zoneRef, isOver, dropped } = useDropZone<HTMLDivElement>()
  const consumedRef = useRef<number>(-1)

  /* ── 列举目录 ───────────────────────────────── */
  const load = useCallback(
    async (path: string) => {
      setLoading(true)
      setError(null)
      try {
        const list = await sftpApi.list(sessionId, path)
        const sorted = [...list].sort((a, b) => {
          if (a.isDirectory !== b.isDirectory) return a.isDirectory ? -1 : 1
          return a.name.localeCompare(b.name)
        })
        setFiles(sorted)
        setCwd(path)
        setPathInput(path)
        setSelected(null)
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e)
        // 结构化错误带原始细节，一并显示，别只留一句「内部错误」。
        const detail = e instanceof BridgeError ? e.detail : undefined
        setError({ message: msg, detail })
        reportError('读取目录失败', e as never)
      } finally {
        setLoading(false)
      }
    },
    [sessionId, reportError],
  )

  // 切换会话时重新列举 —— 旧版依赖数组为空，切会话后仍显示上个会话的文件（缺陷 #8）。
  // 同时跟着会话的工作目录走：打开就落在会话目录，终端里 cd 了也跟着换。
  useEffect(() => {
    void load(sessionCwd || '/')
  }, [load, sessionCwd])

  /* ── 传输进度事件 ───────────────────────────── */
  useEffect(() => {
    return () => {
      // 组件卸载时进度归零，避免切换会话后残留上一个会话的进度条。
    }
  }, [])

  /* ── 上传 ───────────────────────────────────── */
  const upload = useCallback(
    async (localPaths: string[], targetDir: string) => {
      for (const localPath of localPaths) {
        const name = localPath.split(/[\\/]/).pop() || localPath
        const transferId = crypto.randomUUID()
        startTransfer({
          id: transferId,
          sessionId,
          fileName: name,
          transferred: 0,
          total: 0,
          direction: 'upload',
        })
        try {
          // 后端会自动判断是文件还是目录，目录会递归上传。
          await sftpApi.upload(sessionId, localPath, targetDir, transferId)
          finishTransfer(transferId)
        } catch (e) {
          finishTransfer(transferId, e instanceof Error ? e.message : String(e))
          reportError('上传失败', e as never)
        }
      }
      // 上传后刷新当前目录
      await load(cwd)
    },
    [sessionId, cwd, load, startTransfer, finishTransfer, reportError],
  )

  const handlePickFiles = async () => {
    try {
      const picked = await sftpApi.pickFiles()
      if (picked.length > 0) await upload(picked, cwd)
    } catch (e) {
      reportError('选择文件失败', e as never)
    }
  }

  /** 选择一个本地文件夹整体上传（目录会递归上传）。 */
  const handlePickFolder = async () => {
    try {
      const picked = await sftpApi.pickUploadFolder()
      if (picked) await upload([picked], cwd)
    } catch (e) {
      reportError('选择文件夹失败', e as never)
    }
  }

  // 拖放到文件面板：只在松手时上传。
  useEffect(() => {
    if (!dropped || dropped.paths.length === 0) return
    if (consumedRef.current === dropped.seq) return
    consumedRef.current = dropped.seq
    void upload(dropped.paths, cwd)
  }, [dropped, cwd, upload])

  /* ── 下载 ───────────────────────────────────── */
  const handleDownload = async (file: RemoteFile) => {
    try {
      const dir = await sftpApi.pickDirectory()
      if (!dir) return
      const transferId = crypto.randomUUID()
      startTransfer({
        id: transferId,
        sessionId,
        fileName: file.name,
        transferred: 0,
        total: file.size,
        direction: 'download',
      })
      try {
        await sftpApi.download(sessionId, file.path, dir, transferId)
        finishTransfer(transferId)
        push({ kind: 'success', title: '下载完成', message: file.name })
      } catch (e) {
        finishTransfer(transferId, e instanceof Error ? e.message : String(e))
        reportError('下载失败', e as never)
      }
    } catch (e) {
      reportError('选择目录失败', e as never)
    }
  }

  /* ── 删除 ───────────────────────────────────── */
  const handleDelete = (file: RemoteFile) => {
    askConfirm({
      title: file.isDirectory ? 'DELETE DIRECTORY' : 'DELETE FILE',
      message: (
        <>
          {file.isDirectory ? '递归删除目录' : '删除文件'}
          「<span className="confirm-target">{file.name}</span>」？
          <span className="confirm-hint">
            {file.isDirectory
              ? '目录里的全部内容都会被删除，远端无法恢复。'
              : '远端文件无法恢复。'}
          </span>
        </>
      ),
      confirmLabel: '删除',
      danger: true,
      onConfirm: () => {
        void sftpApi
          .remove(sessionId, file.path, file.isDirectory)
          .then(() => {
            push({ kind: 'success', title: '已删除', message: file.name })
            return load(cwd)
          })
          .catch((e) => reportError('删除失败', e as never))
      },
    })
  }

  /* ── 新建目录 ───────────────────────────────── */
  const handleMkdir = async () => {
    const name = window.prompt('新建目录名称')
    if (!name) return
    try {
      await sftpApi.mkdir(sessionId, joinRemote(cwd, name))
      await load(cwd)
    } catch (e) {
      reportError('新建目录失败', e as never)
    }
  }

  const handlePathSubmit = (e: React.FormEvent) => {
    e.preventDefault()
    const p = pathInput.trim() || '/'
    void load(p === '/' ? '/' : p.replace(/\/+$/, ''))
  }

  const sessionTransfers = transfers.filter((t) => t.sessionId === sessionId)

  return (
    <aside className="file-panel">
      <div className="file-panel-head">
        <span className="meta">FILES</span>
        <div className="file-panel-head-actions">
          <button className="icon-btn" onClick={handlePickFiles} title="上传文件">
            <Icon name="upload" size={13} />
          </button>
          <button className="icon-btn" onClick={handlePickFolder} title="上传文件夹">
            <Icon name="folder" size={13} />
          </button>
          <button className="icon-btn" onClick={handleMkdir} title="新建目录">
            <Icon name="folder-open" size={13} />
          </button>
          <button className="icon-btn" onClick={() => void load(cwd)} title="刷新">
            <Icon name="refresh" size={13} />
          </button>
          <button className="icon-btn icon-btn-danger" onClick={onClose} title="关闭面板">
            <Icon name="x" size={13} />
          </button>
        </div>
      </div>

      <div className="file-panel-path">
        <button
          className="icon-btn"
          onClick={() => void load(parentRemote(cwd))}
          disabled={cwd === '/'}
          title="上级目录"
        >
          <Icon name="chevron-left" size={13} />
        </button>
        <form className="file-path-form" onSubmit={handlePathSubmit}>
          <input
            className="input input-mono input-path"
            value={pathInput}
            onChange={(e) => setPathInput(e.target.value)}
            onFocus={(e) => e.currentTarget.select()}
            spellCheck={false}
          />
        </form>
        {offSessionDir && (
          <button
            className="icon-btn"
            onClick={() => void load(sessionCwd as string)}
            title="回到会话所在目录"
          >
            <Icon name="terminal" size={13} />
          </button>
        )}
      </div>

      {sessionCwd && (
        <div className="file-panel-cwd">
          <span className="meta">// SESSION CWD</span>
          <span className="file-cwd-path">{sessionCwd}</span>
        </div>
      )}

      <div
        className={`file-list${isOver ? ' is-drop' : ''}`}
        ref={(el) => {
          zoneRef.current = el
        }}
      >
        {isOver && (
          <div className="file-drop-hint">
            <Icon name="upload" size={20} />
            松开上传到 {cwd}
          </div>
        )}

        {loading ? (
          <div className="file-state">
            <span className="spinner" />
            <span>读取中…</span>
          </div>
        ) : error ? (
          <div className="file-state is-error">
            <Icon name="alert" size={16} />
            <span>{error.message}</span>
            {error.detail && <span className="file-error-detail">{error.detail}</span>}
            <button className="btn btn-ghost btn-xs" onClick={() => void load(cwd)}>
              重试
            </button>
          </div>
        ) : files.length === 0 ? (
          <div className="file-state">
            <Icon name="folder-open" size={18} />
            <span>空目录</span>
          </div>
        ) : (
          <div className="file-rows">
            {files.map((f) => (
              <div
                key={f.path}
                className={`file-row${selected === f.path ? ' is-selected' : ''}`}
                onClick={() => setSelected(f.path)}
                onDoubleClick={() => f.isDirectory && void load(f.path)}
              >
                <Icon
                  name={f.isDirectory ? 'folder' : 'file'}
                  size={13}
                  className={`file-row-icon${f.isDirectory ? ' is-dir' : ''}`}
                />
                <span className="file-row-name">{f.name}</span>
                {!f.isDirectory && (
                  <span className="file-row-size">{formatSize(f.size)}</span>
                )}
                <span className="file-row-time">{formatTime(f.modifiedAt)}</span>
                <div className="file-row-actions">
                  {!f.isDirectory && (
                    <button
                      className="icon-btn"
                      title="下载"
                      onClick={(e) => {
                        e.stopPropagation()
                        void handleDownload(f)
                      }}
                    >
                      <Icon name="download" size={12} />
                    </button>
                  )}
                  <button
                    className="icon-btn icon-btn-danger"
                    title="删除"
                    onClick={(e) => {
                      e.stopPropagation()
                      void handleDelete(f)
                    }}
                  >
                    <Icon name="trash" size={12} />
                  </button>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>

      {sessionTransfers.length > 0 && (
        <div className="file-transfers">
          {sessionTransfers.map((t) => (
            <div className="transfer-item" key={t.id}>
              <div className="transfer-row">
                <Icon
                  name={t.direction === 'upload' ? 'upload' : 'download'}
                  size={11}
                  className="transfer-icon"
                />
                <span className="transfer-name">{t.fileName}</span>
                <span
                  className={`transfer-pct${
                    t.status === 'error' ? ' is-error' : t.status === 'done' ? ' is-done' : ''
                  }`}
                >
                  {t.status === 'error' ? '失败' : t.status === 'done' ? '完成' : `${t.percent}%`}
                </span>
              </div>
              <div className="transfer-bar">
                <div
                  className={`transfer-bar-fill${
                    t.status === 'error' ? ' is-error' : t.status === 'done' ? ' is-done' : ''
                  }`}
                  style={{ width: `${t.percent}%` }}
                />
              </div>
            </div>
          ))}
        </div>
      )}
    </aside>
  )
}
