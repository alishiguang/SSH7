import { openExternal, REPO_URL } from '../api/bridge'
import { Icon } from './Icons'

/**
 * 欢迎屏 / 空状态。
 *
 * 视觉参考 cindy.cn 首屏：网格叠层 + 巨型字水印 + 四角交叉准星 +
 * 红色跑马灯 + 裁切角主按钮。
 */
export default function EmptyState({
  onConnect,
  hostCount,
}: {
  onConnect: () => void
  hostCount: number
}) {
  const tickerItems = [
    'SSH7',
    'SECURE SHELL',
    'RUST CORE',
    'SFTP TRANSFER',
    'MULTI SESSION',
    'OPEN SOURCE',
  ]

  return (
    <div className="empty">
      <div className="empty-grid" aria-hidden="true" />
      <div className="empty-watermark display" aria-hidden="true">
        SSH7
      </div>

      <span className="crosshair tl" aria-hidden="true" />
      <span className="crosshair br" aria-hidden="true" />
      <span className="crosshair tr" aria-hidden="true" />
      <span className="crosshair bl" aria-hidden="true" />

      <div className="empty-inner">
        <div className="empty-tag">
          <span className="empty-tag-line" />
          LESS IS MORE
        </div>

        <h1 className="empty-title display">
          少，就是
          <span className="empty-title-accent">多</span>
          。
        </h1>

        <p className="empty-lead">
          只做两件事：连接，与传文件。终端和 SFTP 全部由 Rust 写就 ——
          没有 Node.js，没有 Chromium，安装包 6 MB 上下。
        </p>

        <div className="empty-actions">
          <button className="btn btn-primary btn-lg" onClick={onConnect}>
            <Icon name="plus" size={13} />
            <span>新建连接</span>
          </button>

          {hostCount > 0 && (
            <div className="empty-hosts-hint">
              <span className="meta">已保存 {hostCount} 台主机</span>
              <span className="empty-hosts-hint-text">从左侧选择即可快速连接</span>
            </div>
          )}
        </div>

        <div className="empty-meta">
          <span>SSH2 / SFTP</span>
          <em>◆</em>
          <span>RUST 2021</span>
          <em>◆</em>
          <span>MACOS · WINDOWS · LINUX</span>
          <em>◆</em>
          <span>MIT LICENSE</span>
          <button
            className="empty-meta-link"
            onClick={() => void openExternal(REPO_URL)}
            title={REPO_URL}
          >
            GITHUB.COM/ALISHIGUANG/SSH7
          </button>
        </div>
      </div>

      <div className="ticker" aria-hidden="true">
        <div className="ticker-track">
          {[0, 1].map((dup) => (
            <div className="ticker-group" key={dup}>
              {tickerItems.map((item) => (
                <span className="ticker-item" key={`${dup}-${item}`}>
                  {item}
                  <em>◆</em>
                </span>
              ))}
            </div>
          ))}
        </div>
      </div>
    </div>
  )
}
