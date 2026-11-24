/**
 * 类型化的 Tauri IPC 桥接层。
 *
 * 所有 `invoke` / `listen` 调用都收敛在这里，组件不直接接触 Tauri API。
 * 这样做的收益：
 * - Rust 端命令改名时，编译期就能发现所有调用点；
 * - 错误统一归一化成 {@link BridgeError}，界面提示逻辑只有一处。
 */

import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

import type {
  AppErrorPayload,
  ConnectRequest,
  CredentialBackend,
  CredentialStoreInfo,
  HostList,
  OpenWindowRequest,
  OpenWindowResult,
  RemoteFile,
  SavedHost,
  SessionClosedEvent,
  SessionCwdEvent,
  SessionDataEvent,
  SessionErrorEvent,
  SessionInfo,
  TestResult,
  TransferProgress,
  WindowAssignment,
} from './types'

/** SSH7 的公开仓库地址。 */
export const REPO_URL = 'https://github.com/alishiguang/SSH7'

/**
 * 用系统默认浏览器打开外部链接。
 *
 * Tauri 里走 `opener` 插件；浏览器预览（`dev-mock.html`）下退化成新窗口，
 * 免得在纯前端环境里直接报错。
 */
export async function openExternal(url: string): Promise<void> {
  const inTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
  if (!inTauri) {
    window.open(url, '_blank', 'noopener,noreferrer')
    return
  }
  try {
    const { openUrl } = await import('@tauri-apps/plugin-opener')
    await openUrl(url)
  } catch {
    // 插件不可用时也别把链接变成死的，退化成新窗口。
    window.open(url, '_blank', 'noopener,noreferrer')
  }
}

/** 归一化后的前端错误：携带 Rust 侧的结构化错误码。 */
export class BridgeError extends Error {
  readonly code: string
  readonly hint?: string
  /** 原始错误文本，排查问题时展示。 */
  readonly detail?: string

  constructor(payload: AppErrorPayload) {
    super(payload.message)
    this.name = 'BridgeError'
    this.code = payload.code
    this.hint = payload.hint
    this.detail = payload.detail
  }
}

/** Rust 侧错误在跨 IPC 之后可能变成普通对象，这里做一次防御性归一化。 */
function toBridgeError(e: unknown): BridgeError {
  if (e instanceof BridgeError) return e
  if (e && typeof e === 'object' && 'code' in e && 'message' in e) {
    const p = e as AppErrorPayload
    return new BridgeError(p)
  }
  if (typeof e === 'string') {
    return new BridgeError({ code: 'INTERNAL', message: e })
  }
  return new BridgeError({ code: 'INTERNAL', message: String(e) })
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(cmd, args)
  } catch (e) {
    throw toBridgeError(e)
  }
}

/**
 * 本窗口的标签。
 *
 * 多窗口下事件必须**定向**投递 —— Rust 侧发的是 `EventTarget::Window{label}`，
 * 只有用**同一个 label** 注册的监听器才收得到（JS `listen` 默认注册成
 * `Any`，那种监听器收不到定向事件）。所以监听时必须把这个标签带上。
 *
 * 取不到就退回到不指定 target（浏览器预览环境）：那边没有多窗口，
 * 收不收得到都一样。
 */
let windowLabelPromise: Promise<string | undefined> | null = null

function currentWindowLabel(): Promise<string | undefined> {
  if (!windowLabelPromise) {
    windowLabelPromise = (async () => {
      if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) {
        return undefined
      }
      try {
        const { getCurrentWindow } = await import('@tauri-apps/api/window')
        return getCurrentWindow().label
      } catch {
        return undefined
      }
    })()
  }
  return windowLabelPromise
}

/**
 * 带窗口标签的 `listen` —— 定向投递的前提。
 *
 * 注意时序：调用方必须等它 resolve 之后再去请求 Rust 补发数据，
 * 否则首批输出会在监听器就位之前送走。
 */
async function listenTo<T>(
  event: string,
  cb: (payload: T) => void,
): Promise<UnlistenFn> {
  const label = await currentWindowLabel()
  return listen<T>(event, (e) => cb(e.payload), label ? { target: label } : undefined)
}

/* ── 主机 ───────────────────────────────────────────── */

export const hostApi = {
  list: () => call<HostList>('host_list'),
  save: (host: SavedHost, secret?: { password?: string; passphrase?: string }) =>
    call<HostList>('host_save', { request: { host, ...secret } }),
  remove: (hostId: string) => call<HostList>('host_delete', { hostId }),
  /** 新建分组（可以是空分组）。 */
  createGroup: (name: string) => call<HostList>('host_group_create', { name }),
  /** 重命名分组：组里的主机一起改到新名字。 */
  renameGroup: (oldName: string, newName: string) =>
    call<HostList>('host_group_rename', { oldName, newName }),
  /** 删除分组：组里的主机回到「未分组」，主机本身不删。 */
  removeGroup: (name: string) => call<HostList>('host_group_remove', { name }),
  /** 重排分组顺序。 */
  reorderGroups: (order: string[]) => call<HostList>('host_groups_reorder', { order }),
  /** 把主机挪到某个分组（不传 group = 回到「未分组」）。 */
  moveHostToGroup: (hostId: string, group?: string) =>
    call<HostList>('host_move_to_group', { hostId, group: group ?? null }),
}

/* ── 会话 ───────────────────────────────────────────── */

export const sessionApi = {
  connect: (req: ConnectRequest) => call<SessionInfo>('session_connect', { request: req }),
  test: (req: ConnectRequest) => call<TestResult>('session_test', { request: req }),
  write: (sessionId: string, data: string) =>
    call<void>('session_write', { sessionId, data }),
  resize: (sessionId: string, cols: number, rows: number) =>
    call<void>('session_resize', { sessionId, cols, rows }),
  /** 前端挂载终端后调用，补发 Rust 侧缓存的首批输出。 */
  attach: (sessionId: string) => call<void>('session_attach', { sessionId }),
  /** 以现有会话为模板克隆一个新会话（凭据在 Rust 侧复用）。 */
  clone: (sessionId: string, label?: string) =>
    call<SessionInfo>('session_clone', { sessionId, label: label ?? null }),
  disconnect: (sessionId: string) => call<void>('session_disconnect', { sessionId }),
}

/* ── SFTP ───────────────────────────────────────────── */

export const sftpApi = {
  list: (sessionId: string, path: string) =>
    call<RemoteFile[]>('sftp_list', { sessionId, path }),
  /** `transferId` 由调用方生成，保证进度事件能对上界面上的传输条目。 */
  upload: (sessionId: string, localPath: string, remoteDir: string, transferId: string) =>
    call<string>('sftp_upload', { sessionId, localPath, remoteDir, transferId }),
  download: (sessionId: string, remotePath: string, localDir: string, transferId: string) =>
    call<string>('sftp_download', { sessionId, remotePath, localDir, transferId }),
  remove: (sessionId: string, remotePath: string, recursive: boolean) =>
    call<void>('sftp_delete', { sessionId, remotePath, recursive }),
  mkdir: (sessionId: string, remotePath: string) =>
    call<void>('sftp_mkdir', { sessionId, remotePath }),
  pickFiles: () => call<string[]>('sftp_pick_files'),
  pickDirectory: () => call<string | null>('sftp_pick_directory'),
  pickUploadFolder: () => call<string | null>('sftp_pick_upload_folder'),
}

/* ── 窗口 ───────────────────────────────────────────── */

export const windowApi = {
  /**
   * 把一台机器开到一个窗口里。
   *
   * 已经开着就聚焦过去，没开着就新开一扇 —— 具体发生了什么看返回值。
   */
  openInWindow: (req: OpenWindowRequest) =>
    call<OpenWindowResult>('session_open_in_window', { request: req }),
  /**
   * 窗口启动时问一次：这扇窗是专门为哪台机器开的？
   * 新窗口靠它自动连上目标主机。
   */
  boot: () => call<WindowAssignment | null>('window_boot'),
}

/* ── 凭据存储 ───────────────────────────────────────── */

export const credentialApi = {
  /** 查询当前用的凭据存储后端。 */
  info: () => call<CredentialStoreInfo>('credential_store_info'),
  /** 切换后端，已保存的秘密会一起搬过去。 */
  set: (backend: CredentialBackend) =>
    call<CredentialStoreInfo>('credential_store_set', { backend }),
}

/* ── 事件 ───────────────────────────────────────────── */

export const EVENT = {
  data: 'session://data',
  closed: 'session://closed',
  cwd: 'session://cwd',
  error: 'session://error',
  transfer: 'transfer://progress',
  focusSession: 'window://focus-session',
} as const

/** 让本窗口切到某个会话标签。 */
export interface FocusSessionEvent {
  sessionId: string
}

export const events = {
  onData: (cb: (e: SessionDataEvent) => void): Promise<UnlistenFn> =>
    listenTo<SessionDataEvent>(EVENT.data, cb),
  onClosed: (cb: (e: SessionClosedEvent) => void): Promise<UnlistenFn> =>
    listenTo<SessionClosedEvent>(EVENT.closed, cb),
  onCwd: (cb: (e: SessionCwdEvent) => void): Promise<UnlistenFn> =>
    listenTo<SessionCwdEvent>(EVENT.cwd, cb),
  onError: (cb: (e: SessionErrorEvent) => void): Promise<UnlistenFn> =>
    listenTo<SessionErrorEvent>(EVENT.error, cb),
  onTransfer: (cb: (e: TransferProgress) => void): Promise<UnlistenFn> =>
    listenTo<TransferProgress>(EVENT.transfer, cb),
  /** 「定位到那扇窗」—— 被聚焦的窗口据此切到对应标签。 */
  onFocusSession: (cb: (e: FocusSessionEvent) => void): Promise<UnlistenFn> =>
    listenTo<FocusSessionEvent>(EVENT.focusSession, cb),
}
