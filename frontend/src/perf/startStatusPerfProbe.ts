interface LoadedStatusPerf {
  startStatusPerf: () => void
}

type StatusPerfLoader = () => Promise<LoadedStatusPerf>
type ErrorReporter = (reason: unknown) => void

const loadStatusPerf: StatusPerfLoader = () => import('./statusPerf')

function reportLoadError(reason: unknown): void {
  console.error('Failed to load status performance probe:', reason)
}

export function startStatusPerfProbe(
  loader: StatusPerfLoader = loadStatusPerf,
  errorReporter: ErrorReporter = reportLoadError,
): void {
  loader()
    .then(({ startStatusPerf }) => startStatusPerf())
    .catch(errorReporter)
}
