import type { MicrophoneSnapshot, MicrophoneTestResult } from '../types'

interface MicrophoneChooserProps {
  snapshot: MicrophoneSnapshot
  test: MicrophoneTestResult | null
  testing: boolean
  onSelect: (id: string | null) => void
  onRefresh: () => void
  onTest: (id: string | null, fallback: boolean) => void
}

export function MicrophoneChooser({
  snapshot,
  test,
  testing,
  onSelect,
  onRefresh,
  onTest,
}: MicrophoneChooserProps) {
  const selectedId = selectedMicrophoneId(snapshot)
  const locked = snapshot.source === 'environment'
  const fallback =
    snapshot.selection.kind === 'missing-with-fallback'
      ? snapshot.selection.fallback
      : snapshot.selection.kind === 'ambiguous-legacy-name'
        ? snapshot.selection.fallback
        : null
  const missing =
    snapshot.selection.kind === 'missing-with-fallback' ||
    snapshot.selection.kind === 'missing-without-fallback'
      ? snapshot.selection.requestedLabel
      : null
  const selectedDevice = selectedId == null
    ? null
    : snapshot.devices.find((device) => device.id === selectedId) ?? null
  const selectedAdvanced = selectedDevice?.tier === 'advanced' ? selectedDevice : null
  const primary = snapshot.devices.filter((device) => device.tier === 'primary')
  const advanced = snapshot.devices.filter(
    (device) => device.tier === 'advanced' && device.id !== selectedAdvanced?.id,
  )

  return (
    <div className="setting-row microphone-setting" data-settings-surface>
      <div>
        <strong>Microphone</strong>
        <span>
          {locked
            ? 'ECHO_MICROPHONE controls this setting.'
            : 'Choose a source by its familiar name. Echo keeps similarly named devices separate.'}
        </span>
      </div>
      <div className="setting-actions microphone-actions">
        <button type="button" className="compact-button" onClick={onRefresh}>Refresh</button>
        {selectedId !== undefined ? (
          <button type="button" className="compact-button" disabled={testing} onClick={() => onTest(selectedId, false)}>
            Test selected
          </button>
        ) : null}
        {fallback ? (
          <button type="button" className="compact-button" disabled={testing} onClick={() => onTest(null, true)}>
            Test system fallback
          </button>
        ) : null}
        {test ? <MicrophoneTestStatus test={test} /> : null}
      </div>
      {missing ? (
        <div className="microphone-warning" role="alert">
          <strong>{missing} is disconnected.</strong>
          <span>
            {fallback
              ? `Recording will use ${fallbackLabel(snapshot, fallback)}.`
              : 'No system fallback is available.'}
          </span>
        </div>
      ) : null}
      {snapshot.selection.kind === 'ambiguous-legacy-name' ? (
        <div className="microphone-warning" role="alert">
          <strong>More than one microphone is named {snapshot.selection.name}.</strong>
          <span>Choose the intended device once to save its stable ID.</span>
        </div>
      ) : null}
      <div className="microphone-options" role="radiogroup" aria-label="Microphone">
        <label className="microphone-option" data-selected={selectedId === null}>
          <input
            type="radio"
            name="microphone"
            checked={selectedId === null}
            disabled={locked}
            onChange={() => onSelect(null)}
          />
          <span>
            <strong>Follow system default</strong>
            <small>{systemDefaultLabel(snapshot)}</small>
          </span>
        </label>
        {selectedAdvanced ? (
          <MicrophoneOption device={selectedAdvanced} selected locked={locked} technical onSelect={onSelect} />
        ) : null}
        {primary.map((device) => (
          <MicrophoneOption
            device={device}
            selected={selectedId === device.id}
            locked={locked}
            onSelect={onSelect}
            key={device.id}
          />
        ))}
        {advanced.length > 0 ? (
          <details className="settings-disclosure microphone-disclosure" data-settings-surface>
            <summary>Advanced audio endpoints <small>{advanced.length}</small></summary>
            <div className="microphone-options disclosure-content">
              {advanced.map((device) => (
                <MicrophoneOption
                  device={device}
                  selected={selectedId === device.id}
                  locked={locked}
                  technical
                  onSelect={onSelect}
                  key={device.id}
                />
              ))}
            </div>
          </details>
        ) : null}
      </div>
      {snapshot.enumerationWarning ? (
        <span className="status-note" data-tone="attention">
          Some microphones could not be listed: {snapshot.enumerationWarning}
        </span>
      ) : null}
    </div>
  )
}

function selectedMicrophoneId(snapshot: MicrophoneSnapshot): string | null | undefined {
  switch (snapshot.selection.kind) {
    case 'system-default':
      return null
    case 'selected':
    case 'legacy-match':
      return snapshot.selection.device.id
    case 'missing-with-fallback':
    case 'missing-without-fallback':
    case 'ambiguous-legacy-name':
      return undefined
  }
}

function systemDefaultLabel(snapshot: MicrophoneSnapshot): string {
  if (snapshot.systemDefault == null) {
    return snapshot.selection.kind === 'system-default' && snapshot.selection.active != null
      ? `Using ${snapshot.selection.active.label} because Linux has no default input`
      : 'No default input'
  }
  return snapshot.systemDefaultIsProxy
    ? 'Follows the current Linux input automatically'
    : `Currently ${snapshot.systemDefault.label}`
}

function fallbackLabel(
  snapshot: MicrophoneSnapshot,
  fallback: MicrophoneSnapshot['devices'][number],
): string {
  return snapshot.systemDefaultIsProxy && fallback.id === snapshot.systemDefault?.id
    ? 'the current input from Linux Sound Settings'
    : `the system fallback, ${fallback.label}`
}

function MicrophoneTestStatus({ test }: { test: MicrophoneTestResult }) {
  const heard = test.kind === 'completed' && test.outcome === 'heard'
  const message = test.kind === 'completed'
    ? test.outcome === 'heard'
      ? `Input heard on ${test.device.label} · level ${test.peakRms.toFixed(3)}`
      : `No input detected on ${test.device.label}`
    : test.device == null
      ? test.message
      : `${test.device.label}: ${test.message}`
  return (
    <span
      className="status-note"
      data-tone={heard ? 'ok' : 'attention'}
      role="status"
      aria-live="polite"
    >
      <span className="status-dot" data-tone={heard ? 'ok' : 'attention'} aria-hidden="true" />
      {message}
    </span>
  )
}

function MicrophoneOption({
  device,
  selected,
  locked,
  technical = false,
  onSelect,
}: {
  device: MicrophoneSnapshot['devices'][number]
  selected: boolean
  locked: boolean
  technical?: boolean
  onSelect: (id: string | null) => void
}) {
  return (
    <label className="microphone-option" data-selected={selected}>
      <input
        type="radio"
        name="microphone"
        checked={selected}
        disabled={locked}
        onChange={() => onSelect(device.id)}
      />
      <span>
        <strong>
          {device.label}
          {device.isDefault ? ' · Current default' : ''}
        </strong>
        <small>{device.hint || 'Audio input'}</small>
        {technical ? <code>{device.id}</code> : null}
      </span>
    </label>
  )
}
