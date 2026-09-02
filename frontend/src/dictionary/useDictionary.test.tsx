import { renderHook, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import {
  addDictionaryEntriesBatch,
  getDictionary,
} from '../tauri'
import { useDictionary } from './useDictionary'

vi.mock('../tauri', () => ({
  addDictionaryEntriesBatch: vi.fn(),
  addDictionaryEntry: vi.fn(),
  getDictionary: vi.fn(() => Promise.resolve([])),
  removeDictionaryEntry: vi.fn(),
}))

describe('useDictionary', () => {
  beforeEach(() => {
    vi.mocked(getDictionary).mockReset()
    vi.mocked(getDictionary).mockResolvedValue([])
    vi.mocked(addDictionaryEntriesBatch).mockReset()
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
})
