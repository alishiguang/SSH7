/**
 * 内联 SVG 图标集。
 *
 * 统一 16×16 视口、`currentColor` 描边，继承父元素的 `color` 与字号，
 * 因此图标颜色永远跟随所在文本的语义色，不需要额外的 class。
 */

export type IconName =
  | 'plus'
  | 'folder'
  | 'folder-open'
  | 'file'
  | 'terminal'
  | 'server'
  | 'chevron-left'
  | 'chevron-right'
  | 'chevron-down'
  | 'x'
  | 'check'
  | 'alert'
  | 'info'
  | 'upload'
  | 'download'
  | 'trash'
  | 'refresh'
  | 'edit'
  | 'search'
  | 'key'
  | 'menu'
  | 'panel'
  | 'arrow-up'
  | 'dots'

const PATHS: Record<IconName, JSX.Element> = {
  plus: <path d="M8 3v10M3 8h10" />,
  folder: <path d="M2 4.5A1.5 1.5 0 0 1 3.5 3h3l1.5 2h4.5A1.5 1.5 0 0 1 14 6.5v5A1.5 1.5 0 0 1 12.5 13h-9A1.5 1.5 0 0 1 2 11.5z" />,
  'folder-open': (
    <>
      <path d="M2 12.5V4.5A1.5 1.5 0 0 1 3.5 3h3l1.5 2h4.5A1.5 1.5 0 0 1 14 6.5v1" />
      <path d="M2.5 12.5 4 8.5h11l-1.6 4a1 1 0 0 1-.94.64H3.5a1 1 0 0 1-.96-1.3z" />
    </>
  ),
  file: (
    <>
      <path d="M9 1.5H4.5A1.5 1.5 0 0 0 3 3v10a1.5 1.5 0 0 0 1.5 1.5h7A1.5 1.5 0 0 0 13 13V5.5z" />
      <path d="M9 1.5V5a.5.5 0 0 0 .5.5H13" />
    </>
  ),
  terminal: (
    <>
      <rect x="1.5" y="2.5" width="13" height="11" rx="1.5" />
      <path d="M4 6.5 6.5 9 4 11.5M8.5 11.5h3.5" />
    </>
  ),
  server: (
    <>
      <rect x="2" y="2.5" width="12" height="4.5" rx="1" />
      <rect x="2" y="9" width="12" height="4.5" rx="1" />
      <path d="M4.5 4.75h.01M4.5 11.25h.01" />
    </>
  ),
  'chevron-left': <path d="M10 3 5 8l5 5" />,
  'chevron-right': <path d="M6 3l5 5-5 5" />,
  'chevron-down': <path d="M3 6l5 5 5-5" />,
  x: <path d="M3.5 3.5l9 9M12.5 3.5l-9 9" />,
  check: <path d="M3 8.5 6.5 12 13 4.5" />,
  alert: (
    <>
      <path d="M8 2 14.5 13.5h-13z" />
      <path d="M8 6.5v3.2M8 11.8h.01" />
    </>
  ),
  info: (
    <>
      <circle cx="8" cy="8" r="6.2" />
      <path d="M8 7.2v4M8 4.8h.01" />
    </>
  ),
  upload: (
    <>
      <path d="M8 10.5V2.5M4.5 6 8 2.5 11.5 6" />
      <path d="M2.5 10v2.5a1 1 0 0 0 1 1h9a1 1 0 0 0 1-1V10" />
    </>
  ),
  download: (
    <>
      <path d="M8 2.5v8M4.5 7 8 10.5 11.5 7" />
      <path d="M2.5 10v2.5a1 1 0 0 0 1 1h9a1 1 0 0 0 1-1V10" />
    </>
  ),
  trash: (
    <>
      <path d="M2.5 4.5h11M6 4.5V3a1 1 0 0 1 1-1h2a1 1 0 0 1 1 1v1.5" />
      <path d="M4 4.5 4.6 13a1 1 0 0 0 1 .95h4.8a1 1 0 0 0 1-.95L12 4.5" />
    </>
  ),
  refresh: (
    <>
      <path d="M13.5 8a5.5 5.5 0 1 1-1.9-4.16" />
      <path d="M13.8 2.5v3.2h-3.2" />
    </>
  ),
  edit: (
    <>
      <path d="M11.2 2.6 13.4 4.8 5.6 12.6 2.5 13.5l.9-3.1z" />
      <path d="M9.8 4l2.2 2.2" />
    </>
  ),
  search: (
    <>
      <circle cx="7.2" cy="7.2" r="4.4" />
      <path d="M10.5 10.5 13.5 13.5" />
    </>
  ),
  key: (
    <>
      <circle cx="5" cy="6" r="2.8" />
      <path d="M7 8 13 13.5M10.5 11l1.4-1.4M12.3 12.8l1.4-1.4" />
    </>
  ),
  menu: <path d="M2.5 4.5h11M2.5 8h11M2.5 11.5h11" />,
  panel: (
    <>
      <rect x="1.5" y="2.5" width="13" height="11" rx="1.5" />
      <path d="M10 2.5v11" />
    </>
  ),
  'arrow-up': <path d="M8 13V3M4 7l4-4 4 4" />,
  dots: (
    <>
      <circle cx="3.5" cy="8" r=".6" fill="currentColor" stroke="none" />
      <circle cx="8" cy="8" r=".6" fill="currentColor" stroke="none" />
      <circle cx="12.5" cy="8" r=".6" fill="currentColor" stroke="none" />
    </>
  ),
}

export function Icon({
  name,
  size = 14,
  className,
}: {
  name: IconName
  size?: number
  className?: string
}) {
  return (
    <svg
      className={className}
      width={size}
      height={size}
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.4"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
    >
      {PATHS[name]}
    </svg>
  )
}
