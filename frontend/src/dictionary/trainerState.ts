import type {
  DictionaryBatchResult,
  DictionaryItem,
  DictionaryTrainingSample,
} from '../generated/ipc'

export const DICTIONARY_TAKE_COUNT = 5

export type PronunciationSample = DictionaryTrainingSample & { take: number }

export type CaptureState =
  | { kind: 'idle' }
  | { kind: 'starting'; target: number }
  | { kind: 'recording'; target: number; captureId: string }
  | { kind: 'finishing'; target: number; captureId: string }

export type TrainerState =
  | { kind: 'entering'; canonical: string }
  | {
      kind: 'collecting'
      canonical: string
      samples: PronunciationSample[]
      capture: CaptureState
      retryTarget: number | null
      error: string | null
    }
  | {
      kind: 'reviewing'
      canonical: string
      samples: PronunciationSample[]
      selected: string[]
      result: DictionaryBatchResult | null
    }
  | {
      kind: 'saving'
      canonical: string
      samples: PronunciationSample[]
      selected: string[]
    }
  | {
      kind: 'save-error'
      canonical: string
      samples: PronunciationSample[]
      selected: string[]
      error: string
    }
  | {
      kind: 'saved'
      canonical: string
      samples: PronunciationSample[]
      result: DictionaryBatchResult
    }

type TrainerEvent =
  | { type: 'canonical-changed'; canonical: string }
  | { type: 'collection-started' }
  | { type: 'collection-resumed' }
  | { type: 'capture-starting'; target: number }
  | { type: 'capture-started'; target: number; captureId: string }
  | { type: 'capture-finishing'; captureId: string }
  | { type: 'capture-failed'; target: number; message: string }
  | {
      type: 'sample-finished'
      captureId: string
      sample: DictionaryTrainingSample
      items: DictionaryItem[]
    }
  | { type: 'selection-toggled'; transcript: string }
  | { type: 'save-started' }
  | { type: 'save-failed'; message: string }
  | { type: 'save-finished'; result: DictionaryBatchResult }
  | { type: 'review-restored' }

export type ReviewStatus = 'actionable' | 'canonical' | 'duplicate' | 'existing' | 'conflict'

export type ReviewedSample = PronunciationSample & {
  status: ReviewStatus
  existingWritten: string | null
}

function cleanPhrase(value: string): string {
  return value.trim().split(/\s+/).filter(Boolean).join(' ')
}

export function dictionaryPhraseKey(value: string): string {
  return cleanPhrase(value).toLocaleLowerCase()
}

export function classifySamples(
  samples: PronunciationSample[],
  canonical: string,
  items: DictionaryItem[],
): ReviewedSample[] {
  const canonicalKey = dictionaryPhraseKey(canonical)
  const seen = new Set<string>()
  return samples.map((sample) => {
    const key = dictionaryPhraseKey(sample.transcript)
    const existing = items.filter((item) => dictionaryPhraseKey(item.spoken) === key)
    let status: ReviewStatus
    let existingWritten: string | null = null
    if (key === canonicalKey) {
      status = 'canonical'
    } else if (existing.some((item) => cleanPhrase(item.written) !== cleanPhrase(canonical))) {
      status = 'conflict'
      existingWritten = existing.find(
        (item) => cleanPhrase(item.written) !== cleanPhrase(canonical),
      )?.written ?? null
    } else if (existing.length > 0) {
      status = 'existing'
    } else if (seen.has(key)) {
      status = 'duplicate'
    } else {
      status = 'actionable'
    }
    seen.add(key)
    return { ...sample, status, existingWritten }
  })
}

export function uniqueReviewSamples(reviewed: ReviewedSample[]): ReviewedSample[] {
  const seen = new Set<string>()
  return reviewed.filter((sample) => {
    const key = dictionaryPhraseKey(sample.transcript)
    if (seen.has(key)) return false
    seen.add(key)
    return true
  })
}

function replaceSample(
  samples: PronunciationSample[],
  target: number,
  sample: DictionaryTrainingSample,
): PronunciationSample[] {
  const completed = { ...sample, take: target + 1 }
  if (target < samples.length) {
    return samples.map((current, index) => index === target ? completed : current)
  }
  if (target === samples.length && samples.length < DICTIONARY_TAKE_COUNT) {
    return [...samples, completed]
  }
  return samples
}

function reviewState(
  canonical: string,
  samples: PronunciationSample[],
  items: DictionaryItem[],
): Extract<TrainerState, { kind: 'reviewing' }> {
  const selected = uniqueReviewSamples(classifySamples(samples, canonical, items))
    .filter((sample) => sample.status === 'actionable')
    .map((sample) => sample.transcript)
  return { kind: 'reviewing', canonical, samples, selected, result: null }
}

export function initialTrainerState(): TrainerState {
  return { kind: 'entering', canonical: '' }
}

export function trainerReducer(state: TrainerState, event: TrainerEvent): TrainerState {
  switch (event.type) {
    case 'canonical-changed':
      return state.kind === 'entering' ? { ...state, canonical: event.canonical } : state
    case 'collection-started':
      return state.kind === 'entering' && state.canonical.trim()
        ? {
            kind: 'collecting',
            canonical: state.canonical.trim(),
            samples: [],
            capture: { kind: 'idle' },
            retryTarget: null,
            error: null,
          }
        : state
    case 'collection-resumed':
      return state.kind === 'reviewing' || state.kind === 'save-error'
        ? {
            kind: 'collecting',
            canonical: state.canonical,
            samples: state.samples,
            capture: { kind: 'idle' },
            retryTarget: null,
            error: null,
          }
        : state
    case 'capture-starting':
      return state.kind === 'collecting' && state.capture.kind === 'idle'
        ? {
            ...state,
            capture: { kind: 'starting', target: event.target },
            retryTarget: event.target < state.samples.length ? event.target : null,
            error: null,
          }
        : state
    case 'capture-started':
      return state.kind === 'collecting' &&
        state.capture.kind === 'starting' &&
        state.capture.target === event.target
        ? {
            ...state,
            capture: { kind: 'recording', target: event.target, captureId: event.captureId },
          }
        : state
    case 'capture-finishing':
      return state.kind === 'collecting' &&
        state.capture.kind === 'recording' &&
        state.capture.captureId === event.captureId
        ? { ...state, capture: { ...state.capture, kind: 'finishing' } }
        : state
    case 'capture-failed':
      return state.kind === 'collecting' &&
        state.capture.kind !== 'idle' &&
        state.capture.target === event.target
        ? { ...state, capture: { kind: 'idle' }, retryTarget: null, error: event.message }
        : state
    case 'sample-finished': {
      if (state.kind !== 'collecting' ||
        (state.capture.kind !== 'recording' && state.capture.kind !== 'finishing') ||
        state.capture.captureId !== event.captureId) return state
      const samples = replaceSample(state.samples, state.capture.target, event.sample)
      if (samples.length === DICTIONARY_TAKE_COUNT) {
        return reviewState(state.canonical, samples, event.items)
      }
      return {
        ...state,
        samples,
        capture: { kind: 'idle' },
        retryTarget: null,
        error: null,
      }
    }
    case 'selection-toggled':
      if (state.kind !== 'reviewing') return state
      return {
        ...state,
        result: null,
        selected: state.selected.includes(event.transcript)
          ? state.selected.filter((value) => value !== event.transcript)
          : [...state.selected, event.transcript],
      }
    case 'save-started':
      return state.kind === 'reviewing'
        ? { kind: 'saving', canonical: state.canonical, samples: state.samples, selected: state.selected }
        : state
    case 'save-failed':
      return state.kind === 'saving'
        ? { ...state, kind: 'save-error', error: event.message }
        : state
    case 'save-finished':
      if (state.kind !== 'saving') return state
      if (event.result.conflicts.length > 0) {
        const conflictKeys = new Set(
          event.result.conflicts.map((conflict) => dictionaryPhraseKey(conflict.spoken)),
        )
        return {
          kind: 'reviewing',
          canonical: state.canonical,
          samples: state.samples,
          selected: state.selected.filter((value) => !conflictKeys.has(dictionaryPhraseKey(value))),
          result: event.result,
        }
      }
      return { kind: 'saved', canonical: state.canonical, samples: state.samples, result: event.result }
    case 'review-restored':
      return state.kind === 'save-error'
        ? {
            kind: 'reviewing',
            canonical: state.canonical,
            samples: state.samples,
            selected: state.selected,
            result: null,
          }
        : state
    default: {
      const exhaustive: never = event
      return exhaustive
    }
  }
}
