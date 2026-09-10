import { act, renderHook } from '@testing-library/react'
import { beforeEach, expect, it, vi } from 'vitest'

import { createPreviewDesktopApi } from '../api/previewDesktopApi'
import type { MicrophoneTestResult } from '../generated/ipc'
import { configureDesktopApi } from '../tauri'
import { createDesktopApiMocks, deferred } from '../test/desktopApiHarness'
import { useMicrophoneController } from './useMicrophoneController'

const preview = createPreviewDesktopApi()
const api = createDesktopApiMocks(preview)
const failed: MicrophoneTestResult = { kind: 'failed', category: 'disconnected', device: null, message: 'Input disconnected' }

beforeEach(() => {
  preview.resetPreviewSettings()
  vi.mocked(api.testInputDevice).mockReset()
  vi.mocked(api.testMicrophoneFallback).mockReset()
  vi.mocked(api.getMicrophones).mockReset().mockImplementation(preview.getMicrophones)
  configureDesktopApi(api)
})

it('keeps the newest test pending when an earlier test settles', async () => {
  const first = deferred<MicrophoneTestResult>()
  const second = deferred<MicrophoneTestResult>()
  vi.mocked(api.testInputDevice).mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise)
  const refresh = vi.fn().mockResolvedValue(undefined)
  const onError = vi.fn()
  const { result } = renderHook(() => useMicrophoneController(onError))
  act(() => {
    result.current.testMicrophone('first', false, refresh)
    result.current.testMicrophone('second', false, refresh)
  })
  await act(async () => first.resolve(failed))
  expect(result.current.testingMic).toBe(true)
  expect(result.current.micTest).toBeNull()
  expect(refresh).not.toHaveBeenCalled()
  await act(async () => second.resolve({ ...failed, message: 'Current input failure' }))
  expect(result.current.testingMic).toBe(false)
  expect(result.current.micTest).toMatchObject({ message: 'Current input failure' })
  expect(refresh).toHaveBeenCalledExactlyOnceWith(true)
})

it('uses the fallback command and refreshes after a rejected fallback test', async () => {
  vi.mocked(api.testMicrophoneFallback).mockRejectedValueOnce(new Error('Fallback disconnected'))
  const refresh = vi.fn().mockResolvedValue(undefined)
  const onError = vi.fn()
  const { result } = renderHook(() => useMicrophoneController(onError))
  await act(async () => result.current.testMicrophone('selected', true, refresh))
  expect(api.testMicrophoneFallback).toHaveBeenCalledOnce()
  expect(api.testInputDevice).not.toHaveBeenCalled()
  expect(onError).toHaveBeenCalledWith(new Error('Fallback disconnected'))
  expect(refresh).toHaveBeenCalledWith(true)
  expect(result.current.testingMic).toBe(false)
})

it('does not run refresh or error callbacks after test disposal', async () => {
  const pending = deferred<MicrophoneTestResult>()
  vi.mocked(api.testInputDevice).mockReturnValueOnce(pending.promise)
  const refresh = vi.fn().mockResolvedValue(undefined)
  const onError = vi.fn()
  const { result, unmount } = renderHook(() => useMicrophoneController(onError))
  act(() => result.current.testMicrophone(null, false, refresh))
  unmount()
  await act(async () => { pending.reject(new Error('Late failure')); await pending.promise.catch(() => undefined) })
  expect(refresh).not.toHaveBeenCalled()
  expect(onError).not.toHaveBeenCalled()
})

it('preserves the newest snapshot when overlapping refreshes settle out of order', async () => {
  const initial = await preview.getMicrophones()
  const older = deferred<typeof initial>()
  const newer = deferred<typeof initial>()
  vi.mocked(api.getMicrophones).mockReturnValueOnce(older.promise).mockReturnValueOnce(newer.promise)
  const onError = vi.fn()
  const { result } = renderHook(() => useMicrophoneController(onError))
  let first: Promise<void>
  let second: Promise<void>
  act(() => { first = result.current.refresh(); second = result.current.refresh() })
  await act(async () => { newer.resolve({ ...initial, revision: 20 }); await second })
  await act(async () => { older.resolve({ ...initial, revision: 10 }); await first })
  expect(result.current.microphones?.revision).toBe(20)
})
