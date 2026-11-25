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

        <div className="empty-poem">
          <div className="empty-poem-head">
            <span className="empty-poem-title">无长物</span>
            <span className="empty-poem-author">白居易</span>
          </div>
          <div className="empty-poem-body">
            <span>莫讶家居<span className="mark">窄</span>，</span>
            <span>无嫌活计<span className="mark">贫</span>。</span>
            <span>只缘<span className="mark">无长物</span>，</span>
            <span>始得作闲人。</span>
            <span>青竹<span className="mark">单</span>床簟，</span>
            <span>乌纱<span className="mark">独</span>幅巾。</span>
            <span>其馀<span className="mark">皆</span>称是，</span>
            <span>亦<span className="mark">足</span>奉吾身。</span>
          </div>
        </div>

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
            GITHUB.COM/CHUAKER7/SSH7
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
