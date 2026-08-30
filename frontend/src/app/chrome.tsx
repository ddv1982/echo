import type { AppStatus } from '../generated/ipc'

export function BrandMark() {
  return (
    <svg viewBox="0 0 1024 1024" className="brand-mark" aria-hidden="true">
      <defs>
        <linearGradient id="brand-gradient" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" className="brand-stop-from" />
          <stop offset="1" className="brand-stop-to" />
        </linearGradient>
      </defs>
      <g fill="url(#brand-gradient)">
        <rect x="176" y="380" width="96" height="200" rx="48" />
        <rect x="320" y="290" width="96" height="380" rx="48" />
        <rect x="464" y="180" width="96" height="600" rx="48" />
        <rect x="608" y="290" width="96" height="380" rx="48" />
        <rect x="752" y="380" width="96" height="200" rx="48" />
        <circle cx="512" cy="856" r="48" />
      </g>
    </svg>
  )
}

// The mark reduced to its bars, for empty states, in the theme's tertiary
// text color.
export function BarsMotif() {
  return (
    <svg viewBox="0 0 1024 1024" className="bars-motif" aria-hidden="true">
      <g fill="currentColor">
        <rect x="176" y="380" width="96" height="200" rx="48" />
        <rect x="320" y="290" width="96" height="380" rx="48" />
        <rect x="464" y="180" width="96" height="600" rx="48" />
        <rect x="608" y="290" width="96" height="380" rx="48" />
        <rect x="752" y="380" width="96" height="200" rx="48" />
        <circle cx="512" cy="856" r="48" />
      </g>
    </svg>
  )
}

export function StatusPill({ status }: { status: AppStatus }) {
  const tone = status.recording
    ? 'recording'
    : status.phase.startsWith('Failed')
      ? 'error'
      : status.phase === 'Idle'
        ? 'ready'
        : 'busy'
  return (
    <div className="status-pill" data-tone={tone} aria-label={`Echo status: ${status.phase}`}>
      <span className="status-dot" aria-hidden="true" />
      {status.phase}
    </div>
  )
}

export function ViewHeader({ title, subtitle }: { title: string; subtitle: string }) {
  return <header className="view-header"><h2>{title}</h2><p>{subtitle}</p></header>
}

export function SectionHeading({ title, subtitle }: { title: string; subtitle: string }) {
  return <div className="section-heading"><h3>{title}</h3><p>{subtitle}</p></div>
}

type SettingTone = 'ok' | 'attention'

export function SettingLine({ label, value, tone }: { label: string; value: string; tone?: SettingTone }) {
  return (
    <div className="setting-line">
      <div><strong>{label}</strong><span>{value}</span></div>
      {tone ? (
        <span className="status-note" data-tone={tone}>
          <span className="status-dot" data-tone={tone} aria-hidden="true" />
          {tone === 'ok' ? 'Ready' : 'Needs setup'}
        </span>
      ) : null}
    </div>
  )
}
