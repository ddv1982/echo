import { act, renderHook, waitFor } from '@testing-library/react'
import { expect, it, vi } from 'vitest'

import { useAsyncSubscription } from './useAsyncSubscription'

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

function renderSubscription(
  getRefresh: (event: string) => (() => Promise<() => void>) | null,
  onError = vi.fn(),
) {
  let handler: ((event: string) => void) | null = null
  const subscribe = vi.fn(async (next: (event: string) => void) => {
    handler = next
    return vi.fn()
  })

  renderHook(() => useAsyncSubscription({
    subscribe,
    onEvent: vi.fn(),
    getRefresh,
    onError,
  }))

  return {
    emit(event: string) {
      if (!handler) throw new Error('subscription handler is not initialized')
      handler(event)
    },
    onError,
  }
}

it('keeps a refresh in flight until its async commit settles', async () => {
  const commitStarted = deferred<void>()
  const commitSettlement = deferred<void>()
  const firstRefresh = vi.fn(async () => async () => {
    commitStarted.resolve()
    await commitSettlement.promise
  })
  const secondRefresh = vi.fn(async () => () => {})
  const subscription = renderSubscription((event) =>
    event === 'first' ? firstRefresh : secondRefresh)

  act(() => subscription.emit('first'))
  await act(async () => commitStarted.promise)

  await act(async () => {
    subscription.emit('second')
    await Promise.resolve()
    await Promise.resolve()
  })

  expect(secondRefresh).not.toHaveBeenCalled()

  await act(async () => commitSettlement.resolve())
  await waitFor(() => expect(secondRefresh).toHaveBeenCalledOnce())
})

it('runs every queued refresh once in FIFO order with serialized commits', async () => {
  const firstSettlement = deferred<void>()
  const secondSettlement = deferred<void>()
  const order: string[] = []
  const firstRefresh = vi.fn(async () => {
    order.push('refresh:first')
    return async () => {
      order.push('commit:first:start')
      await firstSettlement.promise
      order.push('commit:first:end')
    }
  })
  const secondRefresh = vi.fn(async () => {
    order.push('refresh:second')
    return async () => {
      order.push('commit:second:start')
      await secondSettlement.promise
      order.push('commit:second:end')
    }
  })
  const thirdRefresh = vi.fn(async () => {
    order.push('refresh:third')
    return () => {
      order.push('commit:third')
    }
  })
  const subscription = renderSubscription((event) => {
    if (event === 'first') return firstRefresh
    if (event === 'second') return secondRefresh
    return thirdRefresh
  })

  act(() => subscription.emit('first'))
  await waitFor(() => expect(order).toEqual(['refresh:first', 'commit:first:start']))

  act(() => {
    subscription.emit('second')
    subscription.emit('third')
  })
  expect(secondRefresh).not.toHaveBeenCalled()
  expect(thirdRefresh).not.toHaveBeenCalled()

  await act(async () => firstSettlement.resolve())
  await waitFor(() => expect(secondRefresh).toHaveBeenCalledOnce())
  expect(thirdRefresh).not.toHaveBeenCalled()

  await act(async () => secondSettlement.resolve())
  await waitFor(() => expect(thirdRefresh).toHaveBeenCalledOnce())

  expect(firstRefresh).toHaveBeenCalledOnce()
  expect(secondRefresh).toHaveBeenCalledOnce()
  expect(thirdRefresh).toHaveBeenCalledOnce()
  expect(order).toEqual([
    'refresh:first',
    'commit:first:start',
    'commit:first:end',
    'refresh:second',
    'commit:second:start',
    'commit:second:end',
    'refresh:third',
    'commit:third',
  ])
})

it('reports an async commit rejection while mounted', async () => {
  const failure = new Error('commit failed')
  const refresh = vi.fn(async () => async () => {
    throw failure
  })
  const onError = vi.fn()
  const subscription = renderSubscription(() => refresh, onError)

  act(() => subscription.emit('terminal'))

  await waitFor(() => expect(onError).toHaveBeenCalledWith(failure))
})
