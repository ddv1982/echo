import { createRef } from 'react'
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import type {
  DictionaryBatchResult,
  DictionaryItem,
  DictionaryTrainingSample,
} from '../generated/ipc'
import { DictionaryTrainer } from './DictionaryTrainer'
import {
  classifySamples,
  initialTrainerState,
  trainerReducer,
  type PronunciationSample,
} from './trainerState'

const trainingMocks = vi.hoisted(() => ({
  cancel: vi.fn(() => Promise.resolve(true)),
  finish: vi.fn<() => Promise<DictionaryTrainingSample>>(),
  start: vi.fn<() => Promise<string>>(),
}))

vi.mock('../tauri', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../tauri')>()
  return {
    ...actual,
    cancelDictionaryTrainingSample: trainingMocks.cancel,
    finishDictionaryTrainingSample: trainingMocks.finish,
    startDictionaryTrainingSample: trainingMocks.start,
  }
})

const existing: DictionaryItem[] = [
  { spoken: 'already heard', written: 'Kubernetes', createdAt: 1 },
  { spoken: 'shared sound', written: 'Existing value', createdAt: 2 },
]

function sample(take: number, transcript: string): PronunciationSample {
  return { take, transcript, engine: 'whisper-small' }
}

function requireFixture<T>(value: T | undefined, description: string): T {
  if (value === undefined) throw new Error(`missing test fixture: ${description}`)
  return value
}

function deferred<T>() {
  let resolvePromise: ((value: T | PromiseLike<T>) => void) | undefined
  let rejectPromise: ((reason?: unknown) => void) | undefined
  const promise = new Promise<T>((resolve, reject) => {
    resolvePromise = resolve
    rejectPromise = reject
  })
  return {
    promise,
    resolve(value: T | PromiseLike<T>) {
      if (!resolvePromise) throw new Error('deferred promise is not initialized')
      resolvePromise(value)
    },
    reject(reason?: unknown) {
      if (!rejectPromise) throw new Error('deferred promise is not initialized')
      rejectPromise(reason)
    },
  }
}

async function recordTake(take: number) {
  fireEvent.click(await screen.findByRole('button', { name: `Record take ${take}` }))
  fireEvent.click(await screen.findByRole('button', { name: `Stop take ${take}` }))
}

describe('dictionary voice trainer', () => {
  beforeEach(() => {
    trainingMocks.cancel.mockReset()
    trainingMocks.cancel.mockResolvedValue(true)
    trainingMocks.finish.mockReset()
    trainingMocks.start.mockReset()
    let capture = 0
    trainingMocks.start.mockImplementation(() => Promise.resolve(`capture-${capture += 1}`))
  })

  it('classifies canonical, duplicate, existing, and conflicting results', () => {
    const reviewed = classifySamples([
      sample(1, 'kuber netties'),
      sample(2, 'Kubernetes'),
      sample(3, ' KUBER   NETTIES '),
      sample(4, 'already heard'),
      sample(5, 'shared sound'),
    ], 'Kubernetes', existing)

    expect(reviewed.map(({ status }) => status)).toEqual([
      'actionable',
      'canonical',
      'duplicate',
      'existing',
      'conflict',
    ])
    expect(requireFixture(reviewed[4], 'conflicting reviewed sample').existingWritten)
      .toBe('Existing value')
  })

  it('wraps focus in both directions within the entering dialog', () => {
    const triggerRef = createRef<HTMLButtonElement>()
    render(
      <>
        <button ref={triggerRef} type="button">Voice trigger</button>
        <DictionaryTrainer
          items={[]}
          triggerRef={triggerRef}
          onClose={() => undefined}
          onSave={() => Promise.reject(new Error('not reached'))}
        />
      </>,
    )

    const canonical = screen.getByLabelText('Exact word or phrase')
    expect(canonical).toHaveFocus()
    fireEvent.change(canonical, { target: { value: 'Canonical' } })
    const dialog = screen.getByRole('dialog', { name: 'Teach Echo by voice' })
    const firstControl = screen.getByRole('button', { name: 'Close voice training' })
    const lastControl = screen.getByRole('button', { name: 'Start five takes' })

    firstControl.focus()
    fireEvent.keyDown(dialog, { key: 'Tab', shiftKey: true })
    expect(lastControl).toHaveFocus()

    fireEvent.keyDown(dialog, { key: 'Tab' })
    expect(firstControl).toHaveFocus()
  })

  it('keeps focus in the dialog across trainer control transitions', async () => {
    trainingMocks.finish.mockResolvedValueOnce({
      transcript: 'heard phrase',
      engine: 'whisper-small',
    })
    const triggerRef = createRef<HTMLButtonElement>()
    render(
      <>
        <button ref={triggerRef} type="button">Voice trigger</button>
        <DictionaryTrainer
          items={[]}
          triggerRef={triggerRef}
          onClose={() => undefined}
          onSave={() => Promise.reject(new Error('not reached'))}
        />
      </>,
    )

    fireEvent.change(screen.getByLabelText('Exact word or phrase'), { target: { value: 'Canonical' } })
    const start = screen.getByRole('button', { name: 'Start five takes' })
    start.focus()
    fireEvent.click(start)

    const close = screen.getByRole('button', { name: 'Close voice training' })
    await waitFor(() => expect(close).toHaveFocus())

    const record = screen.getByRole('button', { name: 'Record take 1' })
    record.focus()
    fireEvent.click(record)
    const stop = await screen.findByRole('button', { name: 'Stop take 1' })
    expect(close).toHaveFocus()

    fireEvent.click(stop)
    await screen.findByRole('button', { name: 'Retry take 1' })
    expect(close).toHaveFocus()
  })

  it('collects five non-empty takes and saves one batch of actionable variants', async () => {
    const heard = [
      'kuber netties',
      'Kubernetes',
      'kuber netties',
      'already heard',
      'shared sound',
    ]
    heard.forEach((transcript) => trainingMocks.finish.mockResolvedValueOnce({
      transcript,
      engine: 'whisper-small',
    }))
    const onSave = vi.fn(() => Promise.resolve({
      entries: existing,
      added: 1,
      unchanged: 0,
      conflicts: [],
    }))
    const onClose = vi.fn()
    const triggerRef = createRef<HTMLButtonElement>()
    render(
      <>
        <button ref={triggerRef} type="button">Voice trigger</button>
        <DictionaryTrainer
          items={existing}
          triggerRef={triggerRef}
          onClose={onClose}
          onSave={onSave}
        />
      </>,
    )

    fireEvent.change(screen.getByLabelText('Exact word or phrase'), { target: { value: 'Kubernetes' } })
    fireEvent.click(screen.getByRole('button', { name: 'Start five takes' }))
    for (let take = 1; take <= 5; take += 1) {
      await recordTake(take)
      const transcript = requireFixture(heard[take - 1], `transcript for take ${take}`)
      await screen.findAllByText(transcript, { selector: 'q' })
    }

    expect(screen.getByText('Already correct')).toBeInTheDocument()
    expect(screen.getByText('Already in dictionary')).toBeInTheDocument()
    expect(screen.getByText('Already writes Existing value')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Save 1 pronunciation' }))

    await waitFor(() => expect(onSave).toHaveBeenCalledOnce())
    expect(onSave).toHaveBeenCalledWith('Kubernetes', ['kuber netties'])
    await waitFor(() => expect(onClose).toHaveBeenCalledOnce())
  })

  it('does not advance an empty result and replaces a retried take', async () => {
    trainingMocks.finish
      .mockResolvedValueOnce({ transcript: '   ', engine: 'parakeet-tdt-0.6b-v3' })
      .mockResolvedValueOnce({ transcript: 'first hearing', engine: 'parakeet-tdt-0.6b-v3' })
      .mockResolvedValueOnce({ transcript: 'better hearing', engine: 'parakeet-tdt-0.6b-v3' })
    const triggerRef = createRef<HTMLButtonElement>()
    render(
      <>
        <button ref={triggerRef} type="button">Voice trigger</button>
        <DictionaryTrainer
          items={[]}
          triggerRef={triggerRef}
          onClose={() => undefined}
          onSave={() => Promise.reject(new Error('not reached'))}
        />
      </>,
    )

    fireEvent.change(screen.getByLabelText('Exact word or phrase'), { target: { value: 'Canonical' } })
    fireEvent.click(screen.getByRole('button', { name: 'Start five takes' }))
    await recordTake(1)
    expect(await screen.findByRole('alert')).toHaveTextContent('did not hear a word')
    expect(screen.getByText('0 of 5 captured')).toBeInTheDocument()

    await recordTake(1)
    expect(await screen.findByText('first hearing', { selector: 'q' })).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Retry take 1' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Stop take 1' }))
    expect(await screen.findByText('better hearing', { selector: 'q' })).toBeInTheDocument()
    expect(screen.queryByText('first hearing', { selector: 'q' })).not.toBeInTheDocument()
    expect(screen.getByText('1 of 5 captured')).toBeInTheDocument()
  })

  it('keeps capture errors in the collecting state', () => {
    const collecting = trainerReducer(
      trainerReducer(
        trainerReducer(
          { ...initialTrainerState(), canonical: 'Canonical' },
          { type: 'collection-started' },
        ),
        { type: 'capture-starting', target: 0 },
      ),
      { type: 'capture-failed', target: 0, message: 'microphone busy' },
    )

    expect(collecting).toMatchObject({
      kind: 'collecting',
      samples: [],
      capture: { kind: 'idle' },
      error: 'microphone busy',
    })
  })

  it('cancels a capture that finishes starting after the dialog closes', async () => {
    let resolveStart: ((captureId: string) => void) | undefined
    trainingMocks.start.mockImplementation(() => new Promise((resolve) => {
      resolveStart = resolve
    }))
    const triggerRef = createRef<HTMLButtonElement>()
    const view = render(
      <>
        <button ref={triggerRef} type="button">Voice trigger</button>
        <DictionaryTrainer
          items={[]}
          triggerRef={triggerRef}
          onClose={() => undefined}
          onSave={() => Promise.reject(new Error('not reached'))}
        />
      </>,
    )

    fireEvent.change(screen.getByLabelText('Exact word or phrase'), { target: { value: 'Canonical' } })
    fireEvent.click(screen.getByRole('button', { name: 'Start five takes' }))
    fireEvent.click(screen.getByRole('button', { name: 'Record take 1' }))
    view.unmount()
    resolveStart?.('late-capture')

    await waitFor(() => expect(trainingMocks.cancel).toHaveBeenCalledWith('late-capture'))
  })

  it('closes after an active-capture cancellation rejects', async () => {
    const cancellation = Promise.reject(new Error('capture cancellation failed'))
    cancellation.catch(() => undefined)
    const catchSpy = vi.spyOn(cancellation, 'catch')
    trainingMocks.cancel.mockReturnValueOnce(cancellation)
    const onClose = vi.fn()
    const triggerRef = createRef<HTMLButtonElement>()
    const view = render(
      <>
        <button ref={triggerRef} type="button">Voice trigger</button>
        <DictionaryTrainer
          items={[]}
          triggerRef={triggerRef}
          onClose={onClose}
          onSave={() => Promise.reject(new Error('not reached'))}
        />
      </>,
    )

    fireEvent.change(screen.getByLabelText('Exact word or phrase'), { target: { value: 'Canonical' } })
    fireEvent.click(screen.getByRole('button', { name: 'Start five takes' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Record take 1' }))
    await screen.findByRole('button', { name: 'Stop take 1' })

    fireEvent.click(screen.getByRole('button', { name: 'Close voice training' }))
    expect(onClose).toHaveBeenCalledOnce()
    view.unmount()

    expect(trainingMocks.cancel).toHaveBeenCalledOnce()
    expect(trainingMocks.cancel).toHaveBeenCalledWith('capture-1')
    expect(catchSpy).toHaveBeenCalledOnce()
  })

  it('settles an active-capture cancellation rejection during unmount', async () => {
    const cancellation = Promise.reject(new Error('unmount cancellation failed'))
    cancellation.catch(() => undefined)
    const catchSpy = vi.spyOn(cancellation, 'catch')
    trainingMocks.cancel.mockReturnValueOnce(cancellation)
    const triggerRef = createRef<HTMLButtonElement>()
    const view = render(
      <>
        <button ref={triggerRef} type="button">Voice trigger</button>
        <DictionaryTrainer
          items={[]}
          triggerRef={triggerRef}
          onClose={() => undefined}
          onSave={() => Promise.reject(new Error('not reached'))}
        />
      </>,
    )

    fireEvent.change(screen.getByLabelText('Exact word or phrase'), { target: { value: 'Canonical' } })
    fireEvent.click(screen.getByRole('button', { name: 'Start five takes' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Record take 1' }))
    await screen.findByRole('button', { name: 'Stop take 1' })

    view.unmount()

    await waitFor(() => expect(trainingMocks.cancel).toHaveBeenCalledWith('capture-1'))
    expect(catchSpy).toHaveBeenCalledOnce()
  })

  it('holds focus in the dialog while a stopped take is transcribing and recovers it afterward', async () => {
    const finish = deferred<DictionaryTrainingSample>()
    trainingMocks.finish.mockImplementation(() => finish.promise)
    const onClose = vi.fn()
    const triggerRef = createRef<HTMLButtonElement>()
    render(
      <>
        <button ref={triggerRef} type="button">Voice trigger</button>
        <DictionaryTrainer
          items={[]}
          triggerRef={triggerRef}
          onClose={onClose}
          onSave={() => Promise.reject(new Error('not reached'))}
        />
      </>,
    )

    fireEvent.change(screen.getByLabelText('Exact word or phrase'), { target: { value: 'Canonical' } })
    fireEvent.click(screen.getByRole('button', { name: 'Start five takes' }))
    fireEvent.click(screen.getByRole('button', { name: 'Record take 1' }))
    const stop = await screen.findByRole('button', { name: 'Stop take 1' })
    stop.focus()
    fireEvent.click(stop)

    const dialog = screen.getByRole('dialog')
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Close voice training' })).toBeDisabled()
    })
    expect(dialog).toHaveFocus()
    fireEvent.keyDown(dialog, { key: 'Tab' })
    expect(dialog).toHaveFocus()
    fireEvent.keyDown(dialog, { key: 'Escape' })
    expect(onClose).not.toHaveBeenCalled()

    await act(async () => {
      finish.resolve({ transcript: 'heard phrase', engine: 'whisper-small' })
      await finish.promise
    })
    expect(await screen.findByText('heard phrase', { selector: 'q' })).toBeInTheDocument()
    const close = screen.getByRole('button', { name: 'Close voice training' })
    expect(close).toBeEnabled()
    expect(close).toHaveFocus()
  })

  it('holds focus in the dialog while saving and recovers it after a failed save', async () => {
    for (let take = 1; take <= 5; take += 1) {
      trainingMocks.finish.mockResolvedValueOnce({
        transcript: `heard phrase ${take}`,
        engine: 'whisper-small',
      })
    }
    const saving = deferred<DictionaryBatchResult>()
    const triggerRef = createRef<HTMLButtonElement>()
    render(
      <DictionaryTrainer
        items={[]}
        triggerRef={triggerRef}
        onClose={() => undefined}
        onSave={() => saving.promise}
      />,
    )

    fireEvent.change(screen.getByLabelText('Exact word or phrase'), { target: { value: 'Canonical' } })
    fireEvent.click(screen.getByRole('button', { name: 'Start five takes' }))
    for (let take = 1; take <= 5; take += 1) {
      await recordTake(take)
      await screen.findByText(`heard phrase ${take}`, { selector: 'q' })
    }

    const save = screen.getByRole('button', { name: 'Save 5 pronunciations' })
    save.focus()
    fireEvent.click(save)
    const dialog = screen.getByRole('dialog')
    expect(dialog).toHaveFocus()
    fireEvent.keyDown(dialog, { key: 'Tab' })
    expect(dialog).toHaveFocus()

    await act(async () => {
      saving.reject(new Error('save failed'))
      await saving.promise.catch(() => undefined)
    })
    expect(await screen.findByRole('alert')).toHaveTextContent('save failed')
    const close = screen.getByRole('button', { name: 'Close voice training' })
    expect(close).toBeEnabled()
    expect(close).toHaveFocus()
  })

  it.each(['resolve', 'reject'] as const)(
    'settles a stopped take that %s after unmount without updating the trainer',
    async (outcome) => {
      const finish = deferred<DictionaryTrainingSample>()
      trainingMocks.finish.mockImplementation(() => finish.promise)
      const onClose = vi.fn()
      const triggerRef = createRef<HTMLButtonElement>()
      const view = render(
        <>
          <button ref={triggerRef} type="button">Voice trigger</button>
          <DictionaryTrainer
            items={[]}
            triggerRef={triggerRef}
            onClose={onClose}
            onSave={() => Promise.reject(new Error('not reached'))}
          />
        </>,
      )

      fireEvent.change(screen.getByLabelText('Exact word or phrase'), { target: { value: 'Canonical' } })
      fireEvent.click(screen.getByRole('button', { name: 'Start five takes' }))
      fireEvent.click(screen.getByRole('button', { name: 'Record take 1' }))
      fireEvent.click(await screen.findByRole('button', { name: 'Stop take 1' }))
      await waitFor(() => expect(trainingMocks.finish).toHaveBeenCalledWith('capture-1'))

      view.unmount()
      await act(async () => {
        if (outcome === 'resolve') {
          finish.resolve({ transcript: 'late transcript', engine: 'whisper-small' })
        } else {
          finish.reject(new Error('late transcription failure'))
        }
        await finish.promise.catch(() => undefined)
      })

      expect(trainingMocks.cancel).toHaveBeenCalledWith('capture-1')
      expect(onClose).not.toHaveBeenCalled()
    },
  )

  it.each(['resolve', 'reject'] as const)(
    'settles a save that %s after unmount without closing the trainer',
    async (outcome) => {
      for (let take = 1; take <= 5; take += 1) {
        trainingMocks.finish.mockResolvedValueOnce({
          transcript: `heard phrase ${take}`,
          engine: 'whisper-small',
        })
      }
      const saving = deferred<DictionaryBatchResult>()
      const onSave = vi.fn(() => saving.promise)
      const onClose = vi.fn()
      const triggerRef = createRef<HTMLButtonElement>()
      const view = render(
        <>
          <button ref={triggerRef} type="button">Voice trigger</button>
          <DictionaryTrainer
            items={[]}
            triggerRef={triggerRef}
            onClose={onClose}
            onSave={onSave}
          />
        </>,
      )

      fireEvent.change(screen.getByLabelText('Exact word or phrase'), { target: { value: 'Canonical' } })
      fireEvent.click(screen.getByRole('button', { name: 'Start five takes' }))
      for (let take = 1; take <= 5; take += 1) {
        await recordTake(take)
        await screen.findByText(`heard phrase ${take}`, { selector: 'q' })
      }
      fireEvent.click(screen.getByRole('button', { name: 'Save 5 pronunciations' }))
      await waitFor(() => expect(onSave).toHaveBeenCalledOnce())

      view.unmount()
      await act(async () => {
        if (outcome === 'resolve') {
          saving.resolve({ entries: [], added: 5, unchanged: 0, conflicts: [] })
        } else {
          saving.reject(new Error('late save failure'))
        }
        await saving.promise.catch(() => undefined)
      })

      expect(onClose).not.toHaveBeenCalled()
    },
  )
})
