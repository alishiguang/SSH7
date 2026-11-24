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
  RemoteFile,
  SavedHost,
  SessionClosedEvent,
  SessionCwdEvent,
  SessionDataEvent,
  SessionErrorEvent,
  SessionInfo,
  TestResult,
  TransferProgress,
} from './types'

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

/* ── 主机 ───────────────────────────────────────────── */

export const hostApi = {
  list: () => call<SavedHost[]>('host_list'),
  save: (host: SavedHost, secret?: { password?: string; passphrase?: string }) =>
    call<SavedHost[]>('host_save', { request: { host, ...secret } }),
  remove: (hostId: string) => call<SavedHost[]>('host_delete', { hostId }),
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
  clone: (sessionId: string) => call<SessionInfo>('session_clone', { sessionId }),
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

/* ── 事件 ───────────────────────────────────────────── */

export const EVENT = {
  data: 'session://data',
  closed: 'session://closed',
  cwd: 'session://cwd',
  error: 'session://error',
  transfer: 'transfer://progress',
} as const

export const events = {
  onData: (cb: (e: SessionDataEvent) => void): Promise<UnlistenFn> =>
    listen<SessionDataEvent>(EVENT.data, (e) => cb(e.payload)),
  onClosed: (cb: (e: SessionClosedEvent) => void): Promise<UnlistenFn> =>
    listen<SessionClosedEvent>(EVENT.closed, (e) => cb(e.payload)),
  onCwd: (cb: (e: SessionCwdEvent) => void): Promise<UnlistenFn> =>
    listen<SessionCwdEvent>(EVENT.cwd, (e) => cb(e.payload)),
  onError: (cb: (e: SessionErrorEvent) => void): Promise<UnlistenFn> =>
    listen<SessionErrorEvent>(EVENT.error, (e) => cb(e.payload)),
  onTransfer: (cb: (e: TransferProgress) => void): Promise<UnlistenFn> =>
    listen<TransferProgress>(EVENT.transfer, (e) => cb(e.payload)),
}
