import { SectionHeading, SettingLine, ViewHeader } from '../app/chrome'
import { capitalize } from '../app/formatting'
import { ShortcutRow } from '../shortcuts/ShortcutRow'
import { MicrophoneChooser } from './MicrophoneChooser'
import { SpeechSetupSection } from './SpeechSetupSection'
import { TranscriptionSection } from './TranscriptionSection'
import { useSettingsController } from './useSettingsController'
import type {
  AccelerationSkipReason,
  AppStatus,
  LastRun,
  RecordingPolicy,
  SettingField,
  SettingSource,
} from '../generated/ipc'
import type { ThemeMode } from '../types'

const THEME_OPTIONS: readonly ThemeMode[] = ['system', 'light', 'dark']

const DEFAULT_RECORDING_LIMIT_OPTION = 'default'

function recordingLimitOptions(
  policy: RecordingPolicy,
  field: SettingField<number>,
) {
  const defaultSeconds = policy.defaultSeconds || field.effective
  const presets = policy.presetsSeconds.length > 0 ? policy.presetsSeconds : [field.effective]
  const seconds = presets.filter((value) => value !== defaultSeconds)
  if (
    (field.source === 'env' || field.value != null) &&
    !seconds.includes(field.effective)
  ) {
    seconds.push(field.effective)
  }
  seconds.sort((left, right) => left - right)
  return seconds
    .map((value) => ({ value: String(value), label: formatRecordingLength(value) }))
    .concat({
      value: DEFAULT_RECORDING_LIMIT_OPTION,
      label: `${formatRecordingLength(defaultSeconds)} · Default`,
    })
}

function recordingLimitValue(field: SettingField<number>) {
  return field.source !== 'env' && field.value == null
    ? DEFAULT_RECORDING_LIMIT_OPTION
    : String(field.effective)
}

function formatRecordingLength(seconds: number) {
  if (seconds % 60 !== 0) return `${seconds} ${seconds === 1 ? 'second' : 'seconds'}`
  const minutes = seconds / 60
  return `${minutes} ${minutes === 1 ? 'minute' : 'minutes'}`
}

export function SettingsView({
  status,
  theme,
  onThemeChange,
  onStatusChange,
  onError,
}: {
  status: AppStatus
  theme: ThemeMode
  onThemeChange: (theme: ThemeMode) => void
  onStatusChange: () => Promise<void>
  onError: (message: string) => void
}) {
  const {
    settings,
    microphones,
    inventory,
    languages,
    readiness,
    micTest,
    testingMic,
    repairingLegacyShortcut,
    settingsWritePending,
    gpuDevices,
    gpuPrerequisite,
    nextRun,
    whisper,
    lastUsed,
    parakeetRuns,
    selectEngine,
    enableWhisperGpu,
    updateLanguage,
    updateWhisperModel,
    updateRecordSeconds,
    updateWhisperAcceleration,
    updateWhisperGpuDevice,
    updateHud,
    repairLegacy,
    retryShortcutStatus,
    refreshMicrophones,
    refreshReadiness,
    installGpuPrerequisite,
    refreshGpuDevices,
    selectMicrophone,
    testMicrophone,
    reportSettingsError,
  } = useSettingsController({ onStatusChange, onError })
  const previousRun = status.lastRun ?? lastUsed
  return (
    <div className="view-stack settings-view" data-settings-surface>
      <ViewHeader title="Settings" />
      <TranscriptionSection
        settings={settings}
        inventory={inventory}
        languages={languages}
        readiness={readiness}
        settingsWritePending={settingsWritePending}
        gpuDevices={gpuDevices}
        gpuPrerequisite={gpuPrerequisite}
        nextRun={nextRun}
        whisper={whisper}
        lastUsed={previousRun}
        parakeetRuns={parakeetRuns}
        onSelectEngine={(value) => void selectEngine(value)}
        onEnableWhisperGpu={() => void enableWhisperGpu()}
        onUpdateLanguage={(value) => void updateLanguage(value)}
        onUpdateWhisperModel={(value) => void updateWhisperModel(value)}
        onUpdateWhisperAcceleration={(value) => void updateWhisperAcceleration(value)}
        onUpdateWhisperGpuDevice={(value) => void updateWhisperGpuDevice(value)}
        onInstallGpuPrerequisite={installGpuPrerequisite}
        onRefreshGpuDevices={refreshGpuDevices}
      />

      <section className="panel settings-section" aria-label="Input and controls">
        <SectionHeading title="Input and controls" />
        {microphones ? (
          <MicrophoneChooser
            snapshot={microphones}
            test={micTest}
            testing={testingMic}
            onRefresh={() => {
              refreshMicrophones().catch(reportSettingsError)
            }}
            onSelect={selectMicrophone}
            onTest={testMicrophone}
          />
        ) : (
          <SettingLine label="Microphone" value={status.microphoneReady ? 'Default input available' : 'No default input'} tone={status.microphoneReady ? 'ok' : 'attention'} />
        )}
        {settings ? (
          <SettingSelect
            label="Maximum recording length"
            description="Stops timed recordings and recordings started from the button or shortcut."
            value={recordingLimitValue(settings.recordSeconds)}
            options={recordingLimitOptions(status.recordingPolicy, settings.recordSeconds)}
            source={settings.recordSeconds.source}
            envName="ECHO_RECORD_SECONDS"
            onChange={(value) => void updateRecordSeconds(
              value === DEFAULT_RECORDING_LIMIT_OPTION ? null : Number(value),
            )}
          />
        ) : null}
        <ShortcutRow
          status={status}
          repairing={repairingLegacyShortcut}
          onRepair={() => void repairLegacy()}
          onError={reportSettingsError}
          onRetry={retryShortcutStatus}
        />
      </section>

      <section className="panel settings-section" aria-label="Appearance">
        <SectionHeading title="Appearance" />
        {settings ? (
          <SettingToggle
            label="Recording HUD"
            description="Show the recording capsule while you dictate."
            value={settings.hud.effective}
            source={settings.hud.source}
            envName="ECHO_HUD"
            onChange={(value) => void updateHud(value)}
          />
        ) : null}
        <div className="setting-row">
          <div><strong>Theme</strong><span>Applied to the Echo window only.</span></div>
          <div className="segmented-control" role="group" aria-label="Application theme">
            {THEME_OPTIONS.map((mode) => (
              <button type="button" key={mode} data-active={theme === mode} aria-pressed={theme === mode} onClick={() => onThemeChange(mode)}>{capitalize(mode)}</button>
            ))}
          </div>
        </div>
      </section>

      <section className="panel settings-section" aria-label="Setup and diagnostics">
        <SectionHeading title="Setup and diagnostics" />
        {readiness ? (
          <SpeechSetupSection
            readiness={readiness}
            onRefresh={refreshReadiness}
            onError={reportSettingsError}
          />
        ) : null}
        <SettingLine label="Text insertion" value={status.injectionName} tone={status.injectionReady ? 'ok' : 'attention'} />
        {status.lastError ? (
          <div className="setting-line">
            <div><strong>Last failure</strong><span>{status.lastError}</span></div>
            <span className="status-note" data-tone="attention">
              <span className="status-dot" data-tone="attention" aria-hidden="true" />
              Failed
            </span>
          </div>
        ) : null}
        {previousRun ? (
          <>
            <SettingLine label="Previous transcription" value={`${previousRun.engine} · ${previousRun.inferMs} ms`} />
            {previousRun.performance ? (
              <SettingLine label="Last used processing" value={whisperAccelerationLabel(previousRun.performance)} />
            ) : null}
          </>
        ) : null}
        <SettingLine label="Version" value={status.version} />
        {status.settingsPath ? (
          <p className="settings-path">Saved at <code>{status.settingsPath}</code></p>
        ) : null}
      </section>
    </div>
  )
}

const ACCELERATION_SKIP_COPY: Record<AccelerationSkipReason, string> = {
  runtimeMissing: 'GPU asked for, runtime not installed',
  noDeviceEnumerated: 'GPU asked for, no device found',
  pinnedDeviceAbsent: 'GPU asked for, the selected device is absent',
  deviceQuarantined: 'GPU asked for, the device is disabled after a failure',
  cpuFallbackMissing: 'GPU asked for, the managed CPU runtime it falls back to is missing',
  deviceNotReady: 'GPU asked for, the device did not pass its readiness check',
  recoveredToCpu: 'GPU ran and failed, retried on CPU',
}

function whisperAccelerationLabel(performance: NonNullable<LastRun['performance']>) {
  const ran = [backendLabel(performance.backend)]
  if (performance.device) ran.push(performance.device)
  const skip = performance.accelerationSkip
  if (skip) ran.push(ACCELERATION_SKIP_COPY[skip] ?? 'GPU asked for, unavailable')
  return ran.join(' · ')
}

function backendLabel(backend: 'cpu' | 'cuda' | 'vulkan' | 'openVino' | 'rocm' | 'unknown') {
  if (backend === 'openVino') return 'OpenVINO'
  if (backend === 'rocm') return 'ROCm'
  if (backend === 'unknown') return 'Unknown backend'
  return backend.toUpperCase()
}

function SettingSelect({
  label,
  description,
  value,
  options,
  source,
  envName,
  onChange,
}: {
  label: string
  description: string
  value: string
  options: Array<{ value: string; label: string }>
  source: SettingSource
  envName: string
  onChange: (value: string) => void
}) {
  const locked = source === 'env'
  return (
    <label className="setting-row">
      <div>
        <strong>{label}</strong>
        <span>{locked ? envName : description}</span>
      </div>
      <select
        aria-label={label}
        value={value}
        disabled={locked}
        onChange={(event) => onChange(event.target.value)}
      >
        {options.map((option) => (
          <option key={option.value} value={option.value}>{option.label}</option>
        ))}
      </select>
    </label>
  )
}

function SettingToggle({
  label,
  description,
  value,
  source,
  envName,
  onChange,
}: {
  label: string
  description: string
  value: boolean
  source: SettingSource
  envName: string
  onChange: (value: boolean) => void
}) {
  const locked = source === 'env'
  return (
    <div className="setting-row">
      <div>
        <strong>{label}</strong>
        <span>{locked ? envName : description}</span>
      </div>
      <div className="segmented-control" role="group" aria-label={label}>
        <button type="button" data-active={value} aria-pressed={value} disabled={locked} onClick={() => onChange(true)}>On</button>
        <button type="button" data-active={!value} aria-pressed={!value} disabled={locked} onClick={() => onChange(false)}>Off</button>
      </div>
    </div>
  )
}
