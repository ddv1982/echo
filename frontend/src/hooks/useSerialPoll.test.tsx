import { StrictMode, type PropsWithChildren } from 'react'
import { act, renderHook } from '@testing-library/react'

import { useSerialPoll } from './useSerialPoll'

function StrictModeWrapper({ children }: PropsWithChildren) {
  return <StrictMode>{children}</StrictMode>
}

describe('useSerialPoll', () => {
  beforeEach(() => vi.useFakeTimers())
  afterEach(() => vi.useRealTimers())

  it('starts one request across the Strict Mode effect replay', async () => {
    const request = vi.fn().mockResolvedValue('ready')
    const onResult = vi.fn()

    const { unmount } = renderHook(() => useSerialPoll({
      request,
      onResult,
      intervalMs: 1_000,
    }), { wrapper: StrictModeWrapper })

    expect(request).not.toHaveBeenCalled()
    await act(() => vi.advanceTimersByTimeAsync(0))

    expect(request).toHaveBeenCalledOnce()
    expect(onResult).toHaveBeenCalledWith('ready')
    unmount()
  })
})
