import { act, renderHook, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import type { DictionaryItem } from '../generated/ipc'
import {
  addDictionaryEntriesBatch,
  addDictionaryEntry,
  getDictionary,
} from '../tauri'
import { useDictionary } from './useDictionary'

vi.mock('../tauri', () => ({
  addDictionaryEntriesBatch: vi.fn(),
  addDictionaryEntry: vi.fn(),
  getDictionary: vi.fn(() => Promise.resolve([])),
  removeDictionaryEntry: vi.fn(),
}))

function deferred<T>() {
  let resolvePromise: ((value: T | PromiseLike<T>) => void) | null = null
  const promise = new Promise<T>((resolve) => {
    resolvePromise = resolve
  })
  return {
    promise,
    resolve(value: T | PromiseLike<T>) {
      if (!resolvePromise) throw new Error('deferred promise is not initialized')
      resolvePromise(value)
    },
  }
}

describe('useDictionary', () => {
  beforeEach(() => {
    vi.mocked(getDictionary).mockReset()
    vi.mocked(getDictionary).mockResolvedValue([])
    vi.mocked(addDictionaryEntriesBatch).mockReset()
    vi.mocked(addDictionaryEntry).mockReset()
  })

  it('leaves a rejected trainer batch save to the trainer error boundary', async () => {
    const failure = new Error('batch save failed')
    vi.mocked(addDictionaryEntriesBatch).mockRejectedValueOnce(failure)
    const onError = vi.fn()
    const { result } = renderHook(() => useDictionary(onError))
    await waitFor(() => expect(getDictionary).toHaveBeenCalledOnce())

    await expect(result.current.addBatch('Kubernetes', ['kuber netties'])).rejects.toBe(failure)

    expect(addDictionaryEntriesBatch).toHaveBeenCalledWith('Kubernetes', ['kuber netties'])
    expect(onError).not.toHaveBeenCalled()
  })

  it('does not drop an added row when refresh overlaps add', async () => {
    const added: DictionaryItem = {
      spoken: 'kuber netties',
      written: 'Kubernetes',
      createdAt: 1,
    }
    const pendingRefresh = deferred<DictionaryItem[]>()
    vi.mocked(getDictionary)
      .mockImplementationOnce(() => pendingRefresh.promise)
      .mockResolvedValueOnce([added])
    vi.mocked(addDictionaryEntry).mockResolvedValueOnce(added)
    const onError = vi.fn()
    const { result } = renderHook(() => useDictionary(onError))
    await waitFor(() => expect(getDictionary).toHaveBeenCalledOnce())

    let addition = Promise.resolve()
    act(() => {
      addition = result.current.add('kuber netties', 'Kubernetes')
    })
    expect(addDictionaryEntry).not.toHaveBeenCalled()

    pendingRefresh.resolve([])
    await act(async () => {
      await addition
    })

    expect(addDictionaryEntry).toHaveBeenCalledWith('kuber netties', 'Kubernetes')
    expect(result.current.items).toEqual([added])
    expect(onError).not.toHaveBeenCalled()
  })
})
