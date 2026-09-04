import { SectionHeading, SettingLine } from '../app/chrome'
import { capitalize } from '../app/formatting'
import { formatSize } from '../format'
import type {
  ComponentStatus,
  GpuDevice,
  LanguageOptions,
  LastRun,
  ModelInventory,
  NextSpeechRun,
  Readiness,
  Settings,
  WhisperApplicability,
  WhisperModelInfo,
} from '../generated/ipc'

const ENGINE_LABELS: Record<string, string> = {
  whisper: 'Whisper',
  parakeet: 'Parakeet',
  fake: 'Fake',
}

export function TranscriptionSection({
  settings,
  inventory,
  languages,
  readiness,
  settingsWritePending,
  gpuDevices,
  gpuPrerequisite,
  nextRun,
  whisper,
  lastUsed,
  parakeetRuns,
  onSelectEngine,
  onEnableWhisperGpu,
  onUpdateLanguage,
  onUpdateWhisperModel,
  onUpdateWhisperAcceleration,
  onUpdateWhisperGpuDevice,
  onInstallGpuPrerequisite,
  onRefreshGpuDevices,
}: {
  settings: Settings | null
  inventory: ModelInventory | null
  languages: LanguageOptions | null
  readiness: Readiness | null
  settingsWritePending: boolean
  gpuDevices: GpuDevice[]
  gpuPrerequisite: ComponentStatus | null
  nextRun: NextSpeechRun | null
  whisper: WhisperApplicability | null
  lastUsed: LastRun | null
  parakeetRuns: boolean
  onSelectEngine: (value: string) => void
  onEnableWhisperGpu: () => void
  onUpdateLanguage: (value: string) => void
  onUpdateWhisperModel: (value: string | null) => void
  onUpdateWhisperAcceleration: (value: 'cpu' | 'gpu') => void
  onUpdateWhisperGpuDevice: (value: string) => void
  onInstallGpuPrerequisite: () => void
  onRefreshGpuDevices: () => void
}) {
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
                  aria-pressed={settings.engine.effective === option.value}
                  disabled={settings.engine.source === 'env' || settingsWritePending}
                  onClick={() => onSelectEngine(option.value)}
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
            onChange={(event) => onUpdateWhisperModel(event.target.value || null)}
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
          lastUsed={lastUsed}
          onChange={onUpdateLanguage}
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
                    aria-pressed={settings.whisperAcceleration.effective === option.value}
                    disabled={settings.whisperAcceleration.source === 'env' || settingsWritePending}
                    onClick={() => onUpdateWhisperAcceleration(option.value)}
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
              onInstall={onInstallGpuPrerequisite}
              onRefresh={onRefreshGpuDevices}
              onSelect={onUpdateWhisperGpuDevice}
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
              <button type="button" onClick={onEnableWhisperGpu} disabled={settingsWritePending}>
                Use Whisper with GPU
              </button>
            )}
          </div>
        </div>
      ) : null}
    </section>
  )
}

function NextRunSummary({ nextRun, settings }: { nextRun: NextSpeechRun; settings: Settings }) {
  if (nextRun.kind === 'unavailable') {
    return (
      <div className="speech-summary next-run-summary" data-state="needs-setup">
        <div className="speech-summary-copy">
          <span className="status-dot" data-tone="attention" aria-hidden="true" />
          <div><strong>Next transcription needs setup</strong><span>{nextRun.reason}</span></div>
        </div>
      </div>
    )
  }
  const nextEngine = nextRun.engine
  let engine: string
  let processing: string
  switch (nextEngine.kind) {
    case 'whisper':
      engine = `Whisper · ${nextEngine.model}`
      processing = settings.whisperAcceleration.effective === 'gpu' ? 'GPU preferred' : 'CPU'
      break
    case 'parakeet':
      engine = `Parakeet · ${nextEngine.model}`
      processing = 'Engine-managed processing'
      break
    case 'fake':
      engine = 'Fake test engine'
      processing = 'Engine-managed processing'
      break
    default: {
      const unhandledEngine: never = nextEngine
      throw new Error(`Unsupported speech engine: ${JSON.stringify(unhandledEngine)}`)
    }
  }
  const language = nextRun.language === 'auto' ? 'Automatic language' : nextRun.language.toUpperCase()
  return (
    <div className="speech-summary next-run-summary" data-state="ready">
      <div className="speech-summary-copy">
        <span className="status-dot" data-tone="ok" aria-hidden="true" />
        <div><strong>Next transcription</strong><span>{engine} · {language}</span></div>
      </div>
      <span className="status-note chip">{processing}</span>
    </div>
  )
}

function GpuDeviceRow({
  devices, pinned, disabled, prerequisite, installBusy, onInstall, onRefresh, onSelect,
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
          <span>{unsupported
            ? `${prerequisite.label} is not available on this platform. Transcription stays on the CPU.`
            : installing
              ? `Installing ${prerequisite.label}. Transcription stays on the CPU until it finishes.`
              : `GPU needs ${prerequisite.label}, downloaded once. Transcription stays on the CPU until it is installed.`}</span>
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
        <span>{missing
          ? 'The pinned device is not detected. Transcription stays on the CPU until it returns, and your choice is kept.'
          : devices.length === 0
            ? 'No Vulkan device detected. Transcription stays on the CPU.'
            : 'Which GPU runs Whisper. Automatic picks the first non-software device.'}</span>
      </div>
      <div className="setting-actions">
        <select aria-label="GPU device" value={pinned} disabled={disabled} onChange={(event) => onSelect(event.target.value)}>
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

function selectedModelMeta(models: WhisperModelInfo[], current: string) {
  const model = models.find((candidate) => candidate.name === current)
  if (!model) return null
  return [modelQualitySummary(model), model.family, model.multilingual ? 'multilingual' : 'English-only', model.quantisation ?? 'full precision', formatSize(model.sizeBytes)].join(' · ')
}

function modelOptions(models: WhisperModelInfo[], current: string) {
  const options = models.map((model) => ({
    value: model.name,
    label: [modelQualitySummary(model), model.name, model.multilingual ? 'multilingual' : 'English-only', model.quantisation ?? 'full precision', formatSize(model.sizeBytes)].join(' · '),
  }))
  if (current && !models.some((model) => model.name === current)) {
    options.push({ value: current, label: `${current} · not on disk` })
  }
  return options
}

function modelQualitySummary(model: WhisperModelInfo) {
  switch (model.family) {
    case 'large-v3': return 'Highest accuracy'
    case 'large-v3-turbo': return 'Higher accuracy'
    case 'medium': return 'High accuracy'
    case 'small': return 'Recommended for fast dictation'
    case 'tiny':
    case 'base': return 'Low memory'
    default: return 'Installed model'
  }
}

const COMMON_LANGUAGE_ORDER = ['en', 'de', 'es', 'fr']

function LanguageRow({ languages, settings, lastUsed, onChange }: {
  languages: LanguageOptions
  settings: Settings
  lastUsed: LastRun | null
  onChange: (value: string) => void
}) {
  if (languages.mode === 'parakeet') {
    return <SettingLine label="Language" value={`Automatic across ${languages.options.length} languages · not reported`} />
  }
  if (languages.mode === 'english') return <SettingLine label="Language" value="English" />
  const detected = lastUsed?.language ?? null
  const common = [
    ...COMMON_LANGUAGE_ORDER.flatMap((code) => languages.options.filter((option) => option.code === code)),
    ...(detected && !COMMON_LANGUAGE_ORDER.includes(detected)
      ? languages.options.filter((option) => option.code === detected)
      : []),
  ]
  const all = [...languages.options].sort((a, b) => a.englishName.localeCompare(b.englishName))
  const detectedOption = detected ? languages.options.find((option) => option.code === detected) : null
  const probability = lastUsed?.languageProbability ?? null
  const lowConfidence = probability != null && probability < 0.5
  const confident = probability != null && probability >= 0.8
  return (
    <label className="setting-row">
      <div><strong>Language</strong><span>{overrideHint(settings.language.source, 'ECHO_LANGUAGE', 'Pin a language, or let Whisper detect it.')}</span></div>
      <div className="setting-actions">
        <select aria-label="Language" value={settings.language.effective} disabled={settings.language.source === 'env'} onChange={(event) => onChange(event.target.value)}>
          <option value="auto">Auto · detect language</option>
          <optgroup label="Common">{common.map((option) => <option key={option.code} value={option.code}>{capitalize(option.englishName)}</option>)}</optgroup>
          <optgroup label="All languages">{all.map((option) => <option key={option.code} value={option.code}>{capitalize(option.englishName)}</option>)}</optgroup>
        </select>
        {settings.language.effective === 'auto' && detected ? (
          <span className="status-note chip" data-tone={lowConfidence ? 'attention' : 'ok'}>
            <span className="status-dot" data-tone={lowConfidence ? 'attention' : 'ok'} aria-hidden="true" />
            {[detected, detectedOption ? capitalize(detectedOption.englishName) : null, probability != null ? `p=${probability.toFixed(2)}` : null].filter(Boolean).join(' · ')}
          </span>
        ) : null}
        {settings.language.effective === 'auto' && detected && confident ? (
          <button type="button" className="compact-button" onClick={() => onChange(detected)}>
            Pin {detectedOption ? capitalize(detectedOption.englishName) : detected} for speed
          </button>
        ) : null}
      </div>
    </label>
  )
}

function overrideHint(source: Settings['engine']['source'], envName: string, fallback: string) {
  return source === 'env' ? envName : fallback
}

function overrideHintPlain(source: Settings['engine']['source'], fallback: string) {
  return source === 'env' ? 'Set by environment' : fallback
}
