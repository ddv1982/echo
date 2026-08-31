import { Check, Mic, RotateCcw, Square, X } from 'lucide-react'
import {
  KeyboardEvent,
  RefObject,
  useEffect,
  useMemo,
  useReducer,
  useRef,
} from 'react'

import type {
  DictionaryBatchResult,
  DictionaryItem,
} from '../generated/ipc'
import {
  cancelDictionaryTrainingSample,
  finishDictionaryTrainingSample,
  startDictionaryTrainingSample,
} from '../tauri'
import { messageFrom } from '../app/formatting'
import {
  classifySamples,
  DICTIONARY_TAKE_COUNT,
  dictionaryPhraseKey,
  initialTrainerState,
  trainerReducer,
  uniqueReviewSamples,
  type CaptureState,
  type ReviewedSample,
  type TrainerState,
} from './trainerState'

function statusText(sample: ReviewedSample): string {
  switch (sample.status) {
    case 'actionable': return 'New pronunciation'
    case 'canonical': return 'Already correct'
    case 'duplicate': return 'Duplicate take'
    case 'existing': return 'Already in dictionary'
    case 'conflict': return `Already writes ${sample.existingWritten ?? 'something else'}`
    default: {
      const exhaustive: never = sample.status
      return exhaustive
    }
  }
}

function activeCapture(state: TrainerState): Extract<CaptureState, { kind: 'recording' | 'finishing' }> | null {
  if (state.kind !== 'collecting') return null
  return state.capture.kind === 'recording' || state.capture.kind === 'finishing'
    ? state.capture
    : null
}

export function DictionaryTrainer({
  items,
  triggerRef,
  onClose,
  onSave,
}: {
  items: DictionaryItem[]
  triggerRef: RefObject<HTMLButtonElement | null>
  onClose: () => void
  onSave: (written: string, spoken: string[]) => Promise<DictionaryBatchResult>
}) {
  const [state, dispatch] = useReducer(trainerReducer, undefined, initialTrainerState)
  const stateRef = useRef(state)
  const mountedRef = useRef(true)
  const startPendingRef = useRef(false)
  const dialogRef = useRef<HTMLDivElement>(null)
  const canonicalRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    stateRef.current = state
  }, [state])

  useEffect(() => {
    mountedRef.current = true
    const trigger = triggerRef.current
    canonicalRef.current?.focus()
    return () => {
      mountedRef.current = false
      const capture = activeCapture(stateRef.current)
      if (capture) void cancelDictionaryTrainingSample(capture.captureId)
      trigger?.focus()
    }
  }, [triggerRef])

  useEffect(() => {
    if (state.kind === 'saved') onClose()
  }, [onClose, state.kind])

  const reviewed = useMemo(
    () => state.kind === 'entering'
      ? []
      : classifySamples(state.samples, state.canonical, items),
    [items, state],
  )
  const uniqueReviewed = useMemo(() => uniqueReviewSamples(reviewed), [reviewed])
  const closeBlocked = state.kind === 'saving' ||
    (state.kind === 'collecting' && state.capture.kind === 'finishing')

  const close = () => {
    if (closeBlocked) return
    const capture = activeCapture(state)
    if (capture) void cancelDictionaryTrainingSample(capture.captureId)
    onClose()
  }

  const startSample = async (target: number) => {
    if (startPendingRef.current) return
    startPendingRef.current = true
    dispatch({ type: 'capture-starting', target })
    try {
      const captureId = await startDictionaryTrainingSample()
      if (!mountedRef.current) {
        await cancelDictionaryTrainingSample(captureId)
        return
      }
      dispatch({ type: 'capture-started', target, captureId })
    } catch (reason) {
      if (mountedRef.current) {
        dispatch({ type: 'capture-failed', target, message: messageFrom(reason) })
      }
    } finally {
      startPendingRef.current = false
    }
  }

  const retrySample = (target: number) => {
    if (state.kind !== 'collecting') dispatch({ type: 'collection-resumed' })
    void startSample(target)
  }

  const stopSample = async (captureId: string, target: number) => {
    dispatch({ type: 'capture-finishing', captureId })
    try {
      const sample = await finishDictionaryTrainingSample(captureId)
      if (!sample.transcript.trim()) {
        dispatch({
          type: 'capture-failed',
          target,
          message: 'Echo did not hear a word. Try this take again.',
        })
        return
      }
      dispatch({ type: 'sample-finished', captureId, sample, items })
    } catch (reason) {
      dispatch({ type: 'capture-failed', target, message: messageFrom(reason) })
    }
  }

  const save = async () => {
    if (state.kind !== 'reviewing' || state.selected.length === 0) return
    const canonical = state.canonical
    const selected = state.selected
    dispatch({ type: 'save-started' })
    try {
      dispatch({ type: 'save-finished', result: await onSave(canonical, selected) })
    } catch (reason) {
      dispatch({ type: 'save-failed', message: messageFrom(reason) })
    }
  }

  const handleKeys = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === 'Escape') {
      event.preventDefault()
      close()
      return
    }
    if (event.key !== 'Tab' || !dialogRef.current) return
    const focusable = Array.from(dialogRef.current.querySelectorAll<HTMLElement>(
      'button:not([disabled]), input:not([disabled])',
    ))
    if (focusable.length === 0) return
    const first = focusable[0]
    const last = focusable[focusable.length - 1]
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault()
      last.focus()
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault()
      first.focus()
    }
  }

  const sampleCount = state.kind === 'entering' ? 0 : state.samples.length
  const status = state.kind === 'entering'
    ? 'Enter the exact word or phrase first.'
    : state.kind === 'collecting'
      ? state.capture.kind === 'recording'
        ? `Recording take ${state.capture.target + 1} of ${DICTIONARY_TAKE_COUNT}.`
        : state.capture.kind === 'finishing'
          ? `Transcribing take ${state.capture.target + 1} of ${DICTIONARY_TAKE_COUNT}.`
          : `${sampleCount} of ${DICTIONARY_TAKE_COUNT} takes captured.`
      : state.kind === 'saving'
        ? 'Saving pronunciations.'
        : state.kind === 'saved'
          ? `${state.result.added} pronunciations saved.`
          : 'Review the pronunciations Echo heard.'

  return (
    <div className="trainer-backdrop" role="presentation">
      <div
        className="dictionary-trainer"
        role="dialog"
        aria-modal="true"
        aria-labelledby="trainer-title"
        aria-describedby="trainer-description"
        ref={dialogRef}
        onKeyDown={handleKeys}
      >
        <header className="trainer-header">
          <div>
            <p className="eyebrow">Dictionary training</p>
            <h2 id="trainer-title">Teach Echo by voice</h2>
            <p id="trainer-description">
              Say the same word or phrase five times. Echo will learn the different ways your selected transcription model hears it.
            </p>
          </div>
          <button
            className="icon-button"
            type="button"
            onClick={close}
            aria-label="Close voice training"
            disabled={closeBlocked}
          >
            <X size={18} aria-hidden="true" />
          </button>
        </header>

        <p className="sr-only" aria-live="polite" aria-atomic="true">{status}</p>

        {state.kind === 'entering' ? (
          <form
            className="trainer-entry"
            onSubmit={(event) => {
              event.preventDefault()
              dispatch({ type: 'collection-started' })
            }}
          >
            <label htmlFor="trainer-canonical">Exact word or phrase</label>
            <input
              id="trainer-canonical"
              ref={canonicalRef}
              value={state.canonical}
              onChange={(event) => dispatch({ type: 'canonical-changed', canonical: event.target.value })}
              placeholder="Kubernetes"
              autoComplete="off"
            />
            <p>Echo will always write this exact text.</p>
            <div className="trainer-actions">
              <button className="secondary-button" type="button" onClick={close}>Cancel</button>
              <button className="primary-button" type="submit" disabled={!state.canonical.trim()}>
                Start five takes
              </button>
            </div>
          </form>
        ) : null}

        {state.kind === 'collecting' ? (
          <div className="trainer-collection">
            <div className="trainer-target"><span>Write</span><strong>{state.canonical}</strong></div>
            <div className="trainer-progress-row">
            <span>{state.samples.length} of {DICTIONARY_TAKE_COUNT} captured</span>
              <progress value={state.samples.length} max={DICTIONARY_TAKE_COUNT} aria-label="Voice training progress" />
            </div>
            {state.error ? <div className="trainer-error" role="alert">{state.error}</div> : null}
            <ol className="take-list">
              {Array.from({ length: DICTIONARY_TAKE_COUNT }, (_, index) => {
                const sample = reviewed[index]
                const capture = state.capture.kind !== 'idle' && state.capture.target === index
                  ? state.capture
                  : null
                const isNext = index === state.samples.length
                const disabled = state.capture.kind !== 'idle'
                return (
                  <li className="take-row" key={index} data-state={capture?.kind ?? (sample ? 'complete' : 'waiting')}>
                    <span className="take-number">{sample ? <Check size={15} aria-hidden="true" /> : index + 1}</span>
                    <div className="take-copy">
                      <strong>Take {index + 1}</strong>
                      {sample ? (
                        <>
                          <span>Heard: <q>{sample.transcript}</q></span>
                          <small>{sample.engine} · {statusText(sample)}</small>
                        </>
                      ) : (
                        <span>
                          {capture?.kind === 'starting'
                            ? 'Starting…'
                            : capture?.kind === 'recording'
                              ? 'Listening…'
                              : capture?.kind === 'finishing'
                                ? 'Transcribing…'
                                : isNext ? 'Ready to record' : 'Waiting'}
                        </span>
                      )}
                    </div>
                    {capture?.kind === 'recording' ? (
                      <button className="record-take-button is-recording" type="button" onClick={() => void stopSample(capture.captureId, index)} aria-label={`Stop take ${index + 1}`}>
                        <Square size={14} fill="currentColor" aria-hidden="true" /> Stop
                      </button>
                    ) : capture?.kind === 'finishing' || capture?.kind === 'starting' ? (
                      <button className="record-take-button" type="button" disabled>
                        {capture.kind === 'starting' ? 'Starting…' : 'Transcribing…'}
                      </button>
                    ) : sample ? (
                      <button className="record-take-button" type="button" onClick={() => retrySample(index)} disabled={disabled} aria-label={`Retry take ${index + 1}`}>
                        <RotateCcw size={14} aria-hidden="true" /> Retry
                      </button>
                    ) : isNext ? (
                      <button className="record-take-button" type="button" onClick={() => void startSample(index)} disabled={disabled} aria-label={`Record take ${index + 1}`}>
                        <Mic size={15} aria-hidden="true" /> Record
                      </button>
                    ) : null}
                  </li>
                )
              })}
            </ol>
          </div>
        ) : null}

        {state.kind === 'reviewing' || state.kind === 'saving' || state.kind === 'save-error' ? (
          <div className="trainer-review">
            <div className="trainer-target"><span>Always write</span><strong>{state.canonical}</strong></div>
            {state.kind === 'save-error' ? (
              <div className="trainer-error" role="alert">
                {state.error}
                <button type="button" onClick={() => dispatch({ type: 'review-restored' })}>Try again</button>
              </div>
            ) : null}
            {state.kind === 'reviewing' && state.result?.conflicts.length ? (
              <div className="trainer-conflicts" role="alert">
                <strong>Nothing was changed because these pronunciations already have another meaning.</strong>
                {state.result.conflicts.map((conflict) => (
                  <span key={`${conflict.spoken}-${conflict.written}`}>
                    <q>{conflict.spoken}</q> currently writes <q>{conflict.written}</q>.
                  </span>
                ))}
              </div>
            ) : null}
            <fieldset disabled={state.kind !== 'reviewing'}>
              <legend>Pronunciations to add</legend>
              <div className="review-list">
                {uniqueReviewed.map((sample) => {
                  const selectable = sample.status === 'actionable'
                  const selected = state.selected.includes(sample.transcript)
                  return (
                    <label className="review-row" key={dictionaryPhraseKey(sample.transcript)} data-status={sample.status}>
                      <input
                        type="checkbox"
                        checked={selectable && selected}
                        disabled={!selectable || state.kind !== 'reviewing'}
                        onChange={() => dispatch({ type: 'selection-toggled', transcript: sample.transcript })}
                      />
                      <span><q>{sample.transcript}</q><small>{statusText(sample)}</small></span>
                      <em>{sample.engine}</em>
                    </label>
                  )
                })}
              </div>
            </fieldset>
            <div className="trainer-actions trainer-review-actions">
              <button className="secondary-button" type="button" onClick={() => dispatch({ type: 'collection-resumed' })} disabled={state.kind === 'saving'}>
                Retake samples
              </button>
              {state.selected.length === 0 ? (
                <button className="primary-button" type="button" onClick={close}>Done</button>
              ) : (
                <button className="primary-button" type="button" onClick={() => void save()} disabled={state.kind !== 'reviewing'}>
                  {state.kind === 'saving' ? 'Saving…' : `Save ${state.selected.length} pronunciation${state.selected.length === 1 ? '' : 's'}`}
                </button>
              )}
            </div>
          </div>
        ) : null}
      </div>
    </div>
  )
}
