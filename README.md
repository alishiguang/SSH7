# SSH7

> 简洁、酷炫、跨平台的 SSH 客户端 —— SSH 与 SFTP 全部用 Rust 实现。

<div align="center">

<img src="icon-source.png" width="128" alt="SSH7" />

**Ink 黑 · Cream 米白 · 信号红 #F70121**

多会话终端 · SFTP 文件管理 · 拖拽传输 · 密码存本地加密凭据库

[MIT 协议](LICENSE) · macOS / Windows / Linux

📖 **[使用手册](docs/使用手册.md)** —— 全部功能与故障排查

</div>

---

## 为什么是 SSH7

SSH7 是 [LuciSSHClient](https://github.com/ai-anunnaki/LuciSSHClient)（Electron + React + `ssh2`）的 Rust 重写版。

原版能用，但把整个 Node.js 运行时和 Chromium 一起塞进了一个桌面客户端。
SSH7 把 **SSH 协议、SFTP、密钥解析、凭据存储全部搬到 Rust**：

| | LuciSSHClient | SSH7 |
|---|---|---|
| 核心 | Node.js `ssh2` | Rust `russh` + `russh-sftp` |
| 壳 | Electron（~150MB） | Tauri v2（~5MB） |
| 密码存储 | 浏览器 `localStorage` 明文 | 本地加密凭据库 / 系统钥匙串 |
| 主机密钥校验 | 无 | `known_hosts` TOFU + 变更即拒绝 |

## 功能

- **多会话标签页** —— 同时管理多个 SSH 连接，标签切换互不干扰
- **标签改名与排序** —— 双击标签就地改名，按住横向拖动即可调整顺序
- **主机分组** —— 侧边栏按分组归拢，分组可以新建、改名、删除，还能先建好空分组再往里放机器
- **会话去重** —— 点击已存在会话的主机直接切换过去，不会重复开新连接
- **显式新开** —— 主机行悬停出现「＋」，随时为同一主机另开一个会话
- **右键克隆** —— 在会话上右键「克隆为新会话」，凭据在 Rust 侧复用，密码不经前端
- **完整终端** —— xterm.js，256 色、可调整尺寸、链接识别
- **SFTP 文件管理** —— 目录浏览、上传、下载、新建目录、**递归删除**
- **拖拽上传** —— 把本地文件拖进终端区或文件面板即可上传
- **两种认证** —— 密码 / 私钥（含加密私钥口令）
- **凭据存储可选** —— 默认用应用目录内的 AES-256-GCM 加密文件，**绝不落盘明文**；
  也可切到系统钥匙串（macOS Keychain / Windows Credential Manager / Linux Secret Service），设置面板一键切换
  （Windows / Linux 上钥匙串可用时默认用钥匙串，不可用时自动退回加密文件）
- **主机密钥校验** —— 首次连接记录到 `~/.ssh/known_hosts`，之后密钥变更直接拒绝连接
- **实时工作目录** —— 自动跟踪远端 `pwd`，bash / zsh / fish 通用
- **连通性测试** —— 建连前先测一次，显示握手耗时
- **字号与界面缩放** —— 终端字号 9~24px、界面缩放 85%~130%，`Ctrl` `+` / `−` / `0` 随时调整
- **递归上传目录** —— 拖文件夹进去即可，远端自动建出同样的目录结构

## 界面

视觉语言参考 [cindy.cn](https://cindy.cn/)：Ink 黑底、奶白文字、单一信号红强调色，
斜切角按钮、等宽元数据标签、发丝分隔线、网格叠层与红色跑马灯。

## 安装

### macOS

下载 [Releases](https://github.com/alishiguang/SSH7/releases) 里的 `.dmg`，拖入「应用程序」。

### 从源码构建

需要 [Rust](https://rustup.rs/) 1.82+、Node.js 20+。

```bash
git clone https://github.com/alishiguang/SSH7.git
cd SSH7
npm install

# 开发模式
npm run tauri dev

# 打包（当前平台）
npm run tauri build
```

产物在 `src-tauri/target/release/bundle/`。

> **macOS 依赖**：russh 使用 `ring` 作为加密后端，只需要 C 编译器，不需要 cmake。
> `ssh-keygen` 可选，用于把传统 PEM 私钥自动转换成 OpenSSH 格式。

## 架构

```
src/                     React + xterm.js 前端（只负责渲染与输入）
  api/bridge.ts          类型化的 Tauri IPC 封装
  api/types.ts           与 Rust DTO 一一对应的类型
  store/                 zustand 状态：会话 / 主机 / 传输 / 拖放 / 通知
  components/            界面组件
  styles/                设计令牌 + 全局样式 + 组件样式

src-tauri/               Rust 核心
  src/commands/          IPC 边界：参数校验、DTO 映射、错误映射
  src/ssh/
    client.rs            连接生命周期、shell channel、主机密钥校验
    auth.rs              密码 / 私钥解析（含加密私钥）
    sftp.rs              SFTP 操作：列举、传输、递归删除
    osc7.rs              OSC 7 增量解析器（跨 chunk 状态机）
    tracker.rs           按 shell 类型注入工作目录上报钩子
  src/storage/
    hosts.rs             主机列表（不含任何明文秘密）
    secrets.rs           凭据后端门面（钥匙串 / 加密文件，含迁移）
    vault.rs             本地加密凭据库（AES-256-GCM，ring）
    keyring.rs           系统钥匙串读写
  src/state/sessions.rs  会话注册表（创建与销毁统一管理）
```

分层规则：`commands` 是唯一的跨层入口，前端永远拿不到 `russh` 的内部类型。

## 开发

```bash
npm run dev          # 只跑前端（Vite）
npm run tauri dev    # 完整桌面应用
npm run build        # 类型检查 + 前端构建
npm test             # 前端单元测试（vitest）

cd src-tauri
cargo test           # Rust 单元测试
cargo clippy --all-targets
```

持续集成由 GitHub Actions 托管：前端构建与单测、Rust `fmt` / `clippy` / `test`、macOS 打包。
推送 `v*` 标签会自动构建 arm64 / x64 / universal 的 `.dmg` 并发布 Release。

## 测试覆盖

Rust 单元测试（48 项）覆盖了最容易出错的几个地方：

- OSC 7 解析器：序列被 chunk 切开、BEL / ST 双终止符、百分号解码、缓冲区溢出保护
- 远端路径拼接：任意目录层级下的上传目标路径
- 上传计划：单文件、目录递归展开、目录结构按父子顺序创建
- 传输百分比：`total = 0` 时不产生 `NaN`
- shell 钩子：bash 追加而非覆盖 `PROMPT_COMMAND`、zsh 走 `precmd_functions`、fish 走事件
- 主机存储：落盘内容不含任何明文秘密字段
- 本地加密凭据库：存取往返、密文不含明文与账号名、文件权限 0600、
  换主密钥解不开、主密钥丢失后旧密文不可读

真实服务器端到端测试（默认 `#[ignore]`，需显式指定服务器才跑）：

- `tests/sftp_e2e.rs` —— SFTP 列目录、相对路径解析、递归上传、递归删除、下载字节级一致
- `tests/credential_e2e.rs` —— 凭据库存取、显式切换后端时的迁移、惰性兜底迁移，
  以及用库里取出的密码真的连上服务器

前端单元测试（vitest，23 项）覆盖会话状态机、显示偏好与拖放路由：

- 会话状态机：连接中/已连接的会话会被 `findByTarget` 命中（点击主机不重复开新会话），
  已断开与失败的会话不再阻挡新会话，以及标签关闭后的焦点回落
- 显示偏好：字号与界面缩放的夹取、取整、快捷键微调与恢复默认
- 拖放路由：落点命中判定，决定一次拖放归哪个区域处理

## 文档

- **[使用手册](docs/使用手册.md)** —— 面向使用者的完整功能说明与故障排查
- [设计规格](docs/superpowers/specs/2026-11-24-ssh7-design.md) —— 关键技术决策与 18 项缺陷修复对照表

## 安全说明

- 密码与私钥口令**绝不落盘明文**：默认保存在应用目录内 AES-256-GCM 加密的
  凭据库（`vault.bin` + 0600 权限的主密钥 `vault.key`），也可切换到系统钥匙串；
  配置文件 `hosts.json` 里只有主机元数据
- macOS 上未签名的应用每次升级都会被系统钥匙串重新索要授权，因此 macOS
  默认改用本地加密凭据库，不会再反复弹窗
- 主机密钥走 `known_hosts` 校验：首次连接记录，之后密钥变更拒绝连接
- 前端开启严格 CSP，不加载任何远程脚本
- 私钥内容只在 Rust 内存中处理，不会写进日志或错误消息

## 致谢

- [LuciSSHClient](https://github.com/ai-anunnaki/LuciSSHClient) —— 原始实现
- [russh](https://github.com/Eugeny/russh) / [russh-sftp](https://github.com/Eugeny/russh-sftp) —— Rust SSH 实现
- [Tauri](https://tauri.app/) —— 桌面应用框架
- [cindy.cn](https://cindy.cn/) —— 视觉语言参考

## 许可证

[MIT](LICENSE)
