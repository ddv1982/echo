import { SectionHeading, SettingLine, ViewHeader } from '../app/chrome'
import { capitalize } from '../app/formatting'
import { formatSize } from '../format'
import { ShortcutRow } from '../shortcuts/ShortcutRow'
import { MicrophoneChooser } from './MicrophoneChooser'
import { SpeechSetupSection } from './SpeechSetupSection'
import { useSettingsController } from './useSettingsController'
import type {
  AccelerationSkipReason,
  AppStatus,
  ComponentStatus,
  GpuDevice,
  LanguageOptions,
  LastRun,
  NextSpeechRun,
  RecordingPolicy,
  SettingField,
  SettingSource,
  Settings as AppSettings,
  WhisperModelInfo,
} from '../generated/ipc'
import type { ThemeMode } from '../types'

const ENGINE_LABELS: Record<string, string> = {
  whisper: 'Whisper',
  parakeet: 'Parakeet',
  fake: 'Fake',
}

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
  } = useSettingsController({ onStatusChange, onError })
  const previousRun = status.lastRun ?? lastUsed
  const resolvedWhisperModel =
    nextRun?.kind === 'ready' && nextRun.engine.kind === 'whisper'
      ? nextRun.engine.model
      : null
  const canChooseWhisperModel =
    resolvedWhisperModel != null ||
    (nextRun?.kind === 'unavailable' &&
      (settings?.engine.effective === 'whisper' || settings?.engine.effective === 'auto'))
  const selectedWhisperModelMeta = inventory && settings
    ? selectedModelMeta(
        inventory.whisper,
        resolvedWhisperModel ?? settings.whisperModel.effective,
      )
    : null
  const gpuTransitionOverride =
    settings?.engine.source === 'env' && settings.engine.effective !== 'whisper'
    ? 'ECHO_ENGINE'
    : settings?.whisperAcceleration.source === 'env' &&
        settings.whisperAcceleration.effective !== 'gpu'
      ? 'ECHO_WHISPER_ACCELERATION'
      : null

  return (
    <div className="view-stack settings-view" data-settings-surface>
      <ViewHeader title="Settings" subtitle="Change how Echo records and transcribes, on this machine." />
      <section className="panel settings-section" aria-label="Transcription">
        <SectionHeading title="Transcription" subtitle="Choose what Echo will use for the next recording." />
        {settings && nextRun ? (
          <NextRunSummary nextRun={nextRun} settings={settings} />
        ) : null}
        {settings ? (
          <>
            <div className="setting-row">
              <div>
                <strong>Speech engine</strong>
                <span>{overrideHint(settings.engine.source, 'ECHO_ENGINE', 'Automatic shows the engine Echo has resolved for the next recording.')}</span>
              </div>
              <div className="segmented-control" role="group" aria-label="Speech engine">
                {[{ value: 'auto', label: 'Automatic' }]
                  .concat(
                    (inventory?.engines ?? []).map((engine) => ({
                      value: engine.id,
                      label: ENGINE_LABELS[engine.id] ?? engine.id,
                    })),
                  )
                  .map((option) => (
                  <button
                    type="button"
                    key={option.value}
                    data-active={settings.engine.effective === option.value}
                    disabled={settings.engine.source === 'env' || settingsWritePending}
                    onClick={() => void selectEngine(option.value)}
                  >
                    {option.label}
                  </button>
                  ))}
              </div>
            </div>
            {inventory?.engines
              .filter((engine) => !engine.available && engine.id !== 'fake')
              .map((engine) => (
                <div className="setting-row" key={engine.id}>
                  <span className="status-note" data-tone="attention">
                    <span className="status-dot" data-tone="attention" aria-hidden="true" />
                    {engine.id === 'parakeet' ? 'Parakeet' : 'Whisper'}: {engine.reason}
                  </span>
                </div>
              ))}
          </>
        ) : null}
        {settings && parakeetRuns ? (
          <div className="setting-row">
            <div>
              <strong>Speech model</strong>
              <span>Parakeet uses one fixed model and chooses language automatically.</span>
              <span className="model-meta">Fixed model · 25 European languages</span>
            </div>
            <span className="status-note chip">Parakeet TDT 0.6B v3</span>
          </div>
        ) : settings && inventory && canChooseWhisperModel ? (
          <div className="setting-row">
            <div>
              <strong>Speech model</strong>
              <span>{overrideHintPlain(
                settings.whisperModel.source,
                resolvedWhisperModel
                  ? `Automatic currently resolves ${resolvedWhisperModel}.`
                  : 'Choose an installed Whisper model to recover transcription.',
              )}</span>
              {selectedWhisperModelMeta ? (
                <span className="model-meta">{selectedWhisperModelMeta}</span>
              ) : null}
            </div>
            <select
              aria-label="Speech model"
              value={settings.whisperModel.effective}
              disabled={settings.whisperModel.source === 'env' || settingsWritePending}
              onChange={(event) => void updateWhisperModel(event.target.value || null)}
            >
              <option value="">
                {resolvedWhisperModel
                  ? `Automatic · currently ${resolvedWhisperModel}`
                  : 'Automatic · best installed'}
              </option>
              {modelOptions(inventory.whisper, settings.whisperModel.effective).map((option) => (
                <option key={option.value} value={option.value}>{option.label}</option>
              ))}
            </select>
          </div>
        ) : null}
        {settings && languages ? (
          <LanguageRow
            languages={languages}
            settings={settings}
            lastUsed={previousRun}
            onChange={(value) => void updateLanguage(value)}
          />
        ) : null}
        {settings && whisper?.kind === 'applicable' ? (
          <>
            <div className="setting-row">
              <div>
                <strong>Whisper performance</strong>
                <span>{overrideHint(settings.whisperAcceleration.source, 'ECHO_WHISPER_ACCELERATION', 'GPU is a preference with automatic CPU fallback when it cannot run.')}</span>
              </div>
              <div className="segmented-control" role="group" aria-label="Whisper acceleration">
                {([{ value: 'cpu', label: 'CPU' }, { value: 'gpu', label: 'GPU' }] as const)
                  .map((option) => (
                    <button
                      type="button"
                      key={option.value}
                      data-active={settings.whisperAcceleration.effective === option.value}
                      disabled={settings.whisperAcceleration.source === 'env' || settingsWritePending}
                      onClick={() => void updateWhisperAcceleration(option.value)}
                    >
                      {option.label}
                    </button>
                  ))}
              </div>
            </div>
            {settings.whisperAcceleration.effective === 'gpu' && readiness ? (
              <GpuDeviceRow
                devices={gpuDevices}
                pinned={settings.whisperGpuDevice.effective}
                disabled={settingsWritePending}
                prerequisite={gpuPrerequisite}
                installBusy={readiness.activeOperation != null}
                onInstall={installGpuPrerequisite}
                onRefresh={refreshGpuDevices}
                onSelect={(value) => void updateWhisperGpuDevice(value)}
              />
            ) : null}
          </>
        ) : null}
        {settings && whisper?.kind === 'deferred' ? (
          <div className="setting-row">
            <div>
              <strong>Whisper performance</strong>
              <span>{whisper.reason}</span>
            </div>
            <div className="setting-actions">
              <span className="status-note chip">
                {settings.whisperAcceleration.effective === 'gpu' ? 'GPU saved' : 'CPU saved'}
              </span>
              {gpuTransitionOverride ? (
                <span className="status-note chip">{gpuTransitionOverride}</span>
              ) : (
                <button type="button" onClick={() => void enableWhisperGpu()} disabled={settingsWritePending}>
                  Use Whisper with GPU
                </button>
              )}
            </div>
          </div>
        ) : null}
      </section>

      <section className="panel settings-section" aria-label="Input and controls">
        <SectionHeading title="Input and controls" subtitle="Recording source, limits, and shortcuts." />
        {microphones ? (
          <MicrophoneChooser
            snapshot={microphones}
            test={micTest}
            testing={testingMic}
            onRefresh={refreshMicrophones}
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
          onError={onError}
          onRetry={retryShortcutStatus}
        />
      </section>

      <section className="panel settings-section" aria-label="Appearance">
        <SectionHeading title="Appearance" subtitle="Application display." />
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
              <button type="button" key={mode} data-active={theme === mode} onClick={() => onThemeChange(mode)}>{capitalize(mode)}</button>
            ))}
          </div>
        </div>
      </section>

      <section className="panel settings-section" aria-label="Setup and diagnostics">
        <SectionHeading title="Setup and diagnostics" subtitle="Installed components and evidence from previous recordings." />
        {readiness ? (
          <SpeechSetupSection readiness={readiness} onRefresh={refreshReadiness} onError={onError} />
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

function NextRunSummary({
  nextRun,
  settings,
}: {
  nextRun: NextSpeechRun
  settings: AppSettings
}) {
  if (nextRun.kind === 'unavailable') {
    return (
      <div className="speech-summary next-run-summary" data-state="needs-setup">
        <div className="speech-summary-copy">
          <span className="status-dot" data-tone="attention" aria-hidden="true" />
          <div>
            <strong>Next transcription needs setup</strong>
            <span>{nextRun.reason}</span>
          </div>
        </div>
      </div>
    )
  }
  const engine = nextRun.engine.kind === 'whisper'
    ? `Whisper · ${nextRun.engine.model}`
    : nextRun.engine.kind === 'parakeet'
      ? `Parakeet · ${nextRun.engine.model}`
      : 'Fake test engine'
  const language = nextRun.language === 'auto'
    ? 'Automatic language'
    : nextRun.language.toUpperCase()
  const processing = nextRun.engine.kind === 'whisper'
    ? settings.whisperAcceleration.effective === 'gpu' ? 'GPU preferred' : 'CPU'
    : 'Engine-managed processing'
  return (
    <div className="speech-summary next-run-summary" data-state="ready">
      <div className="speech-summary-copy">
        <span className="status-dot" data-tone="ok" aria-hidden="true" />
        <div>
          <strong>Next transcription</strong>
          <span>{engine} · {language}</span>
        </div>
      </div>
      <span className="status-note chip">{processing}</span>
    </div>
  )
}

function GpuDeviceRow({
  devices,
  pinned,
  disabled,
  prerequisite,
  installBusy,
  onInstall,
  onRefresh,
  onSelect,
}: {
  devices: GpuDevice[]
  pinned: string
  disabled: boolean
  prerequisite: ComponentStatus | null
  installBusy: boolean
  onInstall: () => void
  onRefresh: () => void
  onSelect: (value: string) => void
}) {
  const key = (device: GpuDevice) => `${device.id.deviceUUID}:${device.id.driverUUID}`
  const missing = pinned !== '' && !devices.some((device) => key(device) === pinned)
  if (prerequisite != null) {
    const installing = prerequisite.activity != null
    const unsupported = prerequisite.managed.kind === 'unsupported'
    return (
      <div className="setting-row">
        <div>
          <strong>GPU device</strong>
          <span>
            {unsupported
              ? `${prerequisite.label} is not available on this platform. Transcription stays on the CPU.`
              : installing
                ? `Installing ${prerequisite.label}. Transcription stays on the CPU until it finishes.`
                : `GPU needs ${prerequisite.label}, downloaded once. Transcription stays on the CPU until it is installed.`}
          </span>
        </div>
        <div className="setting-actions">
          {unsupported ? null : (
            <button type="button" onClick={onInstall} disabled={disabled || installBusy || installing}>
              {installing ? 'Installing' : `Install ${prerequisite.label}`}
            </button>
          )}
        </div>
      </div>
    )
  }

  return (
    <div className="setting-row">
      <div>
        <strong>GPU device</strong>
        <span>
          {missing
            ? 'The pinned device is not detected. Transcription stays on the CPU until it returns, and your choice is kept.'
            : devices.length === 0
              ? 'No Vulkan device detected. Transcription stays on the CPU.'
              : 'Which GPU runs Whisper. Automatic picks the first non-software device.'}
        </span>
      </div>
      <div className="setting-actions">
        <select
          aria-label="GPU device"
          value={pinned}
          disabled={disabled}
          onChange={(event) => onSelect(event.target.value)}
        >
          <option value="">Automatic</option>
          {devices.map((device) => (
            <option key={key(device)} value={key(device)}>
              {device.software ? `${device.name} · software` : device.name}
            </option>
          ))}
          {missing ? <option value={pinned}>Pinned device · not detected</option> : null}
        </select>
        <button type="button" onClick={onRefresh} disabled={disabled}>Detect</button>
      </div>
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

function selectedModelMeta(models: WhisperModelInfo[], current: string) {
  const model = models.find((candidate) => candidate.name === current)
  if (!model) return null
  return [
    modelQualitySummary(model),
    model.family,
    model.multilingual ? 'multilingual' : 'English-only',
    model.quantisation ?? 'full precision',
    formatSize(model.sizeBytes),
  ].join(' · ')
}

function modelOptions(models: WhisperModelInfo[], current: string) {
  const options = models.map((model) => ({
    value: model.name,
    label: [
      modelQualitySummary(model),
      model.name,
      model.multilingual ? 'multilingual' : 'English-only',
      model.quantisation ?? 'full precision',
      formatSize(model.sizeBytes),
    ].join(' · '),
  }))
  if (current && !models.some((model) => model.name === current)) {
    options.push({ value: current, label: `${current} · not on disk` })
  }
  return options
}

function modelQualitySummary(model: WhisperModelInfo) {
  switch (model.family) {
    case 'large-v3':
      return 'Highest accuracy'
    case 'large-v3-turbo':
      return 'Higher accuracy'
    case 'medium':
      return 'High accuracy'
    case 'small':
      return 'Recommended for fast dictation'
    case 'tiny':
    case 'base':
      return 'Low memory'
    default:
      return 'Installed model'
  }
}

const COMMON_LANGUAGE_ORDER = ['en', 'de', 'es', 'fr']

function LanguageRow({
  languages,
  settings,
  lastUsed,
  onChange,
}: {
  languages: LanguageOptions
  settings: AppSettings
  lastUsed: LastRun | null
  onChange: (value: string) => void
}) {
  if (languages.mode === 'parakeet') {
    return (
      <SettingLine
        label="Language"
        value={`Automatic across ${languages.options.length} languages · not reported`}
      />
    )
  }
  if (languages.mode === 'english') {
    return <SettingLine label="Language" value="English" />
  }
  const detected = lastUsed?.language ?? null
  const common = [
    ...COMMON_LANGUAGE_ORDER.flatMap((code) =>
      languages.options.filter((option) => option.code === code),
    ),
    ...(detected && !COMMON_LANGUAGE_ORDER.includes(detected)
      ? languages.options.filter((option) => option.code === detected)
      : []),
  ]
  const all = [...languages.options].sort((a, b) => a.englishName.localeCompare(b.englishName))
  const detectedOption = detected
    ? languages.options.find((option) => option.code === detected)
    : null
  const probability = lastUsed?.languageProbability ?? null
  const lowConfidence = probability != null && probability < 0.5
  const confident = probability != null && probability >= 0.8
  return (
    <label className="setting-row">
      <div>
        <strong>Language</strong>
        <span>
          {overrideHint(
            settings.language.source,
            'ECHO_LANGUAGE',
            'Pin a language, or let Whisper detect it.',
          )}
        </span>
      </div>
      <div className="setting-actions">
        <select
          aria-label="Language"
          value={settings.language.effective}
          disabled={settings.language.source === 'env'}
          onChange={(event) => onChange(event.target.value)}
        >
          <option value="auto">Auto · detect language</option>
          <optgroup label="Common">
            {common.map((option) => (
              <option key={option.code} value={option.code}>
                {capitalize(option.englishName)}
              </option>
            ))}
          </optgroup>
          <optgroup label="All languages">
            {all.map((option) => (
              <option key={option.code} value={option.code}>
                {capitalize(option.englishName)}
              </option>
            ))}
          </optgroup>
        </select>
        {settings.language.effective === 'auto' && detected ? (
          <span className="status-note chip" data-tone={lowConfidence ? 'attention' : 'ok'}>
            <span
              className="status-dot"
              data-tone={lowConfidence ? 'attention' : 'ok'}
              aria-hidden="true"
            />
            {[
              detected,
              detectedOption ? capitalize(detectedOption.englishName) : null,
              probability != null ? `p=${probability.toFixed(2)}` : null,
            ]
              .filter(Boolean)
              .join(' · ')}
          </span>
        ) : null}
        {settings.language.effective === 'auto' && detected && confident ? (
          <button
            type="button"
            className="compact-button"
            onClick={() => onChange(detected)}
          >
            Pin {detectedOption ? capitalize(detectedOption.englishName) : detected} for speed
          </button>
        ) : null}
      </div>
    </label>
  )
}

function overrideHint(source: SettingSource, envName: string, fallback: string) {
  return source === 'env' ? envName : fallback
}

function overrideHintPlain(source: SettingSource, fallback: string) {
  return source === 'env' ? 'Set by environment' : fallback
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
        <button type="button" data-active={value} disabled={locked} onClick={() => onChange(true)}>On</button>
        <button type="button" data-active={!value} disabled={locked} onClick={() => onChange(false)}>Off</button>
      </div>
    </div>
  )
}
