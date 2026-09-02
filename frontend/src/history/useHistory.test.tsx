import { act, renderHook, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import type { HistoryItem } from '../generated/ipc'
import { deleteHistoryItem, getHistory } from '../tauri'
import { useHistory } from './useHistory'

vi.mock('../tauri', () => ({
  clearHistory: vi.fn(() => Promise.resolve(0)),
  deleteHistoryItem: vi.fn(() => Promise.resolve(true)),
  getHistory: vi.fn(() => Promise.resolve([])),
}))

function item(id: string): HistoryItem {
  return {
    id,
    text: id,
    raw: id,
    engine: 'fake',
    startedAt: 1,
    inferMs: 2,
    injection: 'Typed',
  }
}

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

describe('useHistory', () => {
  beforeEach(() => {
    vi.mocked(getHistory).mockReset()
    vi.mocked(getHistory).mockResolvedValue([])
    vi.mocked(deleteHistoryItem).mockReset()
    vi.mocked(deleteHistoryItem).mockResolvedValue(true)
  })

  it('waits for a pending refresh before deleting and keeps the deletion', async () => {
    const pendingRefresh = deferred<HistoryItem[]>()
    vi.mocked(getHistory).mockImplementationOnce(() => pendingRefresh.promise)
    const onError = vi.fn()
    const { result } = renderHook(() => useHistory(onError))
    await waitFor(() => expect(getHistory).toHaveBeenCalledOnce())

    let removal = Promise.resolve(false)
    act(() => {
      removal = result.current.remove('old')
    })
    expect(deleteHistoryItem).not.toHaveBeenCalled()

    pendingRefresh.resolve([item('old'), item('new')])
    await act(async () => {
      await removal
    })

    expect(deleteHistoryItem).toHaveBeenCalledWith('old')
    expect(result.current.items).toEqual([item('new')])
    expect(onError).not.toHaveBeenCalled()
  })

  it('removes a stale local row when an idempotent delete returns false', async () => {
    vi.mocked(getHistory).mockResolvedValueOnce([item('old'), item('new')])
    vi.mocked(deleteHistoryItem).mockResolvedValueOnce(false)
    const onError = vi.fn()
    const { result } = renderHook(() => useHistory(onError))
    await waitFor(() => expect(result.current.items).toEqual([item('old'), item('new')]))

    let removed = true
    await act(async () => {
      removed = await result.current.remove('old')
    })

    expect(removed).toBe(false)
    expect(result.current.items).toEqual([item('new')])
  })

  it('preserves local rows and reports a rejected delete', async () => {
    vi.mocked(getHistory).mockResolvedValueOnce([item('old')])
    vi.mocked(deleteHistoryItem).mockRejectedValueOnce(new Error('delete failed'))
    const onError = vi.fn()
    const { result } = renderHook(() => useHistory(onError))
    await waitFor(() => expect(result.current.items).toEqual([item('old')]))

    await act(async () => {
      await result.current.remove('old')
    })

    expect(result.current.items).toEqual([item('old')])
    expect(onError).toHaveBeenCalledWith(expect.objectContaining({ message: 'delete failed' }))
  })
})
