/**
 * 与 Rust 侧 DTO 一一对应的类型定义。
 *
 * Rust 端使用 `#[serde(rename_all = "camelCase")]`，因此这里全部是驼峰命名。
 * 修改任何一侧都必须同步另一侧，否则运行时会出现字段缺失。
 */

export type AuthType = 'password' | 'privateKey'

/** 已保存的主机。**不包含任何秘密**，密码只存在于本机凭据库。 */
export interface SavedHost {
  id: string
  label: string
  /** 所属分组，留空表示不分组。 */
  group?: string
  host: string
  port: number
  username: string
  authType: AuthType
  privateKeyPath?: string
  /** 凭据库中是否存有密码 */
  hasPassword: boolean
  /** 凭据库中是否存有私钥口令 */
  hasPassphrase: boolean
}

/** 建立连接的请求。 */
export interface ConnectRequest {
  label?: string
  /** 保存主机时使用的分组名，留空表示不分组。 */
  group?: string
  host: string
  port: number
  username: string
  authType: AuthType
  password?: string
  privateKeyPath?: string
  passphrase?: string
  /** 是否保存到主机列表 */
  save?: boolean
  /** 保存时用于覆盖已有主机 */
  hostId?: string
}

/** 保存主机时提交的内容。秘密单独传入，不混在主机元数据里。 */
export interface HostSaveRequest {
  host: SavedHost
  password?: string
  passphrase?: string
}

/**
 * 主机列表 + 分组清单。
 *
 * `groups` 里可以有**空分组**（还没放主机），用户可以先把「生产 / 测试」
 * 建好，往里放机器时直接选。
 */
export interface HostList {
  hosts: SavedHost[]
  groups: string[]
}

/** 结构化错误载荷，与 Rust 的 `AppErrorPayload` 对应。 */
export interface AppErrorPayload {
  code: string
  message: string
  hint?: string
  /** 技术细节（原始错误文本），界面以小字展示，排查问题时用。 */
  detail?: string
}

/** 凭据存储后端。 */
export type CredentialBackend = 'keyring' | 'vault'

/** 凭据存储信息，与 Rust 的 `CredentialStoreInfo` 对应。 */
export interface CredentialStoreInfo {
  backend: CredentialBackend
  /** 当前后端的展示名。 */
  backendLabel: string
  /** 系统钥匙串是否可用（Linux 上可能没有 Secret Service）。 */
  keyringAvailable: boolean
  /** 当前后端是不是该平台的默认值。 */
  isDefault: boolean
  /** 该平台的默认后端。 */
  defaultBackend: CredentialBackend
}

/** 连通性测试结果。 */
export interface TestResult {
  ok: boolean
  latencyMs?: number
  error?: AppErrorPayload
}

/** 会话状态。 */
export type SessionStatus = 'connecting' | 'connected' | 'disconnected' | 'failed'

/** 一次成功建立的会话。 */
export interface SessionInfo {
  sessionId: string
  label: string
  host: string
  port: number
  username: string
  /** 本次使用的认证方式 */
  authType: AuthType
}

/** 远端文件条目。 */
export interface RemoteFile {
  name: string
  /** 完整远端路径，便于直接交给下载 / 删除接口 */
  path: string
  isDirectory: boolean
  isSymlink: boolean
  size: number
  /** Unix 秒级时间戳 */
  modifiedAt: number
  permissions?: string
}

/** 传输进度事件。 */
export interface TransferProgress {
  transferId: string
  sessionId: string
  fileName: string
  transferred: number
  total: number
  /** 0..100，已夹取，不会出现 NaN（缺陷 #13） */
  percent: number
  direction: 'upload' | 'download'
}

/** 终端数据事件。 */
export interface SessionDataEvent {
  sessionId: string
  data: string
}

/** 当前目录变化事件。 */
export interface SessionCwdEvent {
  sessionId: string
  cwd: string
}

/** 会话关闭事件。 */
export interface SessionClosedEvent {
  sessionId: string
  reason?: string
}

/** 会话错误事件（连接建立后发生的错误）。 */
export interface SessionErrorEvent {
  sessionId: string
  error: AppErrorPayload
}
