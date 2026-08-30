import { formatSize } from '../format'
import { presentSpeechSetup } from '../setup'
import {
  cancelSetup,
  removeManaged,
  repairManaged,
  startSetup,
  verifyManaged,
} from '../tauri'
import type { Readiness } from '../generated/ipc'

interface SpeechSetupSectionProps {
  readiness: Readiness
  guided?: boolean
  onRefresh: () => void
  onError: (message: string) => void
}

export function SpeechSetupSection({
  readiness,
  guided = false,
  onRefresh,
  onError,
}: SpeechSetupSectionProps) {
  const run = (action: Promise<unknown>) => {
    void action.then(onRefresh).catch((reason: unknown) => onError(messageFrom(reason)))
  }
  const setup = presentSpeechSetup(readiness)
  const { recommended, parakeet } = setup
  const recommendedModel = readiness.components.find(
    (component) => component.id === readiness.recommendedModel,
  )
  const recommendedLabel = recommendedModel?.label ?? 'recommended setup'
  const activeOperation = readiness.activeOperation
  const tone = setup.state.kind === 'ready'
    ? 'ok'
    : setup.state.kind === 'in-progress'
      ? 'progress'
      : 'attention'
  const repairComponent = setup.state.kind === 'needs-repair' ? setup.state.component : null

  return (
    <div className="speech-setup" aria-label="Speech setup">
      <div className="speech-summary" data-state={setup.state.kind}>
        <div className="speech-summary-copy">
          <span className="status-dot" data-tone={tone} aria-hidden="true" />
          <div>
            <strong>{setup.state.title}</strong>
            <span>{setup.state.detail}</span>
          </div>
        </div>
        <div className="setting-actions speech-summary-actions">
          {activeOperation && readiness.activeCancellable ? (
            <button type="button" className="compact-button" onClick={() => run(cancelSetup(activeOperation))}>
              Cancel
            </button>
          ) : activeOperation == null && !readiness.speechReady && readiness.managedSupported && recommended ? (
            <button
              type="button"
              className="primary-button setup-primary"
              disabled={!recommended.diskReady}
              onClick={() => run(startSetup('recommended'))}
            >
              {recommended.satisfied
                ? `Use ${recommendedLabel}`
                : `Set up ${recommendedLabel} · ${formatSize(recommended.downloadBytes)}`}
            </button>
          ) : null}
          {activeOperation == null && !readiness.speechReady && readiness.managedSupported && parakeet ? (
            <button
              type="button"
              className="compact-button"
              disabled={!parakeet.diskReady}
              onClick={() => run(startSetup('parakeet'))}
            >
              Use Parakeet instead
            </button>
          ) : null}
          {activeOperation == null && repairComponent ? (
            <button type="button" className="compact-button" onClick={() => run(repairManaged(repairComponent.id))}>
              Repair
            </button>
          ) : null}
        </div>
        {setup.state.kind === 'in-progress' && setup.state.component?.activity ? (
          <SetupProgress component={setup.state.component} />
        ) : null}
        {recommended && !recommended.diskReady && recommended.diskReason ? (
          <span className="setup-inline-error" role="alert">Recommended: {recommended.diskReason}</span>
        ) : null}
        {parakeet && !parakeet.satisfied && !parakeet.diskReady && parakeet.diskReason ? (
          <span className="setup-inline-error" role="alert">Parakeet: {parakeet.diskReason}</span>
        ) : null}
      </div>
      {!guided ? (
        <>
          <details className="settings-disclosure" data-settings-surface>
            <summary>
              <span>Installed components</span>
              <small>{readiness.components.filter((component) => component.activeOrigin != null).length} available</small>
            </summary>
            <div className="disclosure-content">
              {setup.installedComponents.map((component) => (
                <ComponentMaintenance
                  component={component}
                  activeOperation={activeOperation}
                  managedSupported={readiness.managedSupported}
                  run={run}
                  key={component.id}
                />
              ))}
            </div>
          </details>
          <details className="settings-disclosure" data-settings-surface>
            <summary>Advanced speech options</summary>
            <div className="disclosure-content">
              <p className="disclosure-note">
                System runtimes and manually imported models stay available. Echo never changes external files.
              </p>
              {setup.alternativePlans.map((plan) => (
                <div className="setting-row setup-plan-row" key={plan.id}>
                  <div>
                    <strong>{plan.label}</strong>
                    <span>{plan.satisfied ? 'Ready' : `${formatSize(plan.downloadBytes)} download`}</span>
                  </div>
                  {readiness.managedSupported ? (
                    <button
                      type="button"
                      className="compact-button"
                      disabled={activeOperation != null || !plan.diskReady}
                      onClick={() => run(startSetup(plan.id))}
                    >
                      {plan.satisfied ? 'Use' : 'Install'}
                    </button>
                  ) : null}
                </div>
              ))}
            </div>
          </details>
        </>
      ) : null}
    </div>
  )
}

function SetupProgress({ component }: { component: Readiness['components'][number] }) {
  if (!component.activity) return null
  return (
    <div
      className="download-track setup-progress"
      role="progressbar"
      aria-label={`${component.label} ${component.activity.phase}`}
      aria-valuemin={0}
      aria-valuemax={component.activity.totalBytes}
      aria-valuenow={component.activity.receivedBytes}
    >
      <div
        className="download-fill"
        style={{
          width: `${component.activity.totalBytes > 0
            ? Math.min(100, (component.activity.receivedBytes / component.activity.totalBytes) * 100)
            : 0}%`,
        }}
      />
    </div>
  )
}

function ComponentMaintenance({
  component,
  activeOperation,
  managedSupported,
  run,
}: {
  component: Readiness['components'][number]
  activeOperation: string | null
  managedSupported: boolean
  run: (action: Promise<unknown>) => void
}) {
  return (
    <div className="setting-row component-row">
      <div>
        <strong>{component.label}</strong>
        <span>{managedStateLabel(component)}</span>
        {component.external.map((external) => (
          <span className="offer-url" key={`${external.origin}:${external.path}`}>
            {capitalize(external.origin)} · {external.path}
          </span>
        ))}
      </div>
      <div className="setting-actions">
        {component.activity ? <SetupProgress component={component} /> : null}
        {component.managed.kind === 'needs-repair' ? (
          <>
            <button type="button" className="compact-button" disabled={activeOperation != null} onClick={() => run(repairManaged(component.id))}>
              Repair
            </button>
            <button
              type="button"
              className="compact-button danger-button"
              disabled={activeOperation != null}
              onClick={() => {
                if (window.confirm(`Remove damaged Echo-managed ${component.label}? External files stay untouched.`)) {
                  run(removeManaged(component.id))
                }
              }}
            >
              Remove damaged copy
            </button>
          </>
        ) : null}
        {component.managed.kind === 'absent' && managedSupported ? (
          <button type="button" className="compact-button" disabled={activeOperation != null} onClick={() => run(repairManaged(component.id))}>
            {component.managed.resumableBytes > 0
              ? `Resume · ${formatSize(component.managed.resumableBytes)} saved`
              : component.external.length > 0
                ? 'Install managed copy'
                : 'Install'}
          </button>
        ) : null}
        {component.managed.kind === 'ready' ? (
          <>
            <button type="button" className="compact-button" disabled={activeOperation != null} onClick={() => run(verifyManaged(component.id))}>
              Verify
            </button>
            <button
              type="button"
              className="compact-button danger-button"
              disabled={activeOperation != null}
              onClick={() => {
                if (window.confirm(`Remove Echo-managed ${component.label}? External files stay untouched.`)) {
                  run(removeManaged(component.id))
                }
              }}
            >
              Remove · {formatSize(component.managed.bytes)}
            </button>
          </>
        ) : null}
      </div>
    </div>
  )
}

function managedStateLabel(component: Readiness['components'][number]) {
  if (component.activity) {
    if (component.activity.phase === 'downloading') {
      const percent = component.activity.totalBytes > 0
        ? Math.floor((component.activity.receivedBytes / component.activity.totalBytes) * 100)
        : 0
      return component.activity.resumedFromBytes > 0
        ? `Downloading ${percent}% · resumed at ${formatSize(component.activity.resumedFromBytes)}`
        : `Downloading ${percent}%`
    }
    return `${capitalize(component.activity.phase)}…`
  }
  switch (component.managed.kind) {
    case 'absent':
      if (component.activeOrigin === 'system') return 'Ready · system runtime'
      if (component.activeOrigin === 'external') return 'Ready · manually installed'
      return component.managed.resumableBytes > 0
        ? `Partial · ${formatSize(component.managed.resumableBytes)} ready to resume`
        : 'Not installed by Echo'
    case 'ready':
      return `Ready · managed by Echo · ${component.managed.version}`
    case 'needs-repair':
      return component.activeOrigin === 'external' || component.activeOrigin === 'system'
        ? `Managed copy needs repair · using ${component.activeOrigin} fallback`
        : `Needs repair · ${component.managed.reason}`
    case 'unsupported':
      return component.managed.reason
  }
}

function capitalize(value: string) {
  return value.charAt(0).toUpperCase() + value.slice(1)
}

function messageFrom(reason: unknown) {
  return reason instanceof Error ? reason.message : String(reason)
}
