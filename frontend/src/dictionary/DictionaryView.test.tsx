import { act, fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { deferred } from '../test/desktopApiHarness'
import { DictionaryView } from './DictionaryView'

function pendingForm() {
  const pending = deferred<void>()
  const onAdd = vi.fn(() => pending.promise)
  const onError = vi.fn()
  render(<DictionaryView items={[]} onAdd={onAdd} onAddBatch={vi.fn()} onRemove={vi.fn()} onError={onError} />)
  const spoken = screen.getByLabelText('What Echo hears')
  const written = screen.getByLabelText('What Echo should write')
  fireEvent.change(spoken, { target: { value: 'old spoken' } })
  fireEvent.change(written, { target: { value: 'old written' } })
  const submit = screen.getByRole('button', { name: 'Add' })
  const form = submit.closest('form')!
  fireEvent.submit(form)
  return { pending, onAdd, onError, spoken, written, form }
}

describe('dictionary draft saving', () => {
  it('clears the submitted draft when there were no further edits', async () => {
    const { pending, spoken, written, onAdd } = pendingForm()
    await act(async () => pending.resolve())
    expect(onAdd).toHaveBeenCalledWith('old spoken', 'old written')
    expect(spoken).toHaveValue('')
    expect(written).toHaveValue('')
  })

  it.each(['spoken', 'written', 'both', 'edit-back'] as const)('preserves a draft edited in %s while saving', async (edit) => {
    const { pending, spoken, written } = pendingForm()
    if (edit !== 'written') fireEvent.change(spoken, { target: { value: 'new spoken' } })
    if (edit === 'written' || edit === 'both') fireEvent.change(written, { target: { value: 'new written' } })
    if (edit === 'edit-back') fireEvent.change(spoken, { target: { value: 'old spoken' } })
    await act(async () => pending.resolve())
    expect(spoken).toHaveValue(edit === 'spoken' || edit === 'both' ? 'new spoken' : 'old spoken')
    expect(written).toHaveValue(edit === 'written' || edit === 'both' ? 'new written' : 'old written')
  })

  it('retains edited input and reports a failed save', async () => {
    const { pending, spoken, written, onError } = pendingForm()
    fireEvent.change(spoken, { target: { value: 'new spoken' } })
    await act(async () => pending.reject(new Error('Disk full')))
    expect(spoken).toHaveValue('new spoken')
    expect(written).toHaveValue('old written')
    expect(onError).toHaveBeenCalledWith('Disk full')
  })

  it('rejects repeated submits while a write is pending', async () => {
    const { pending, form, onAdd } = pendingForm()
    fireEvent.submit(form)
    fireEvent.submit(form)
    expect(onAdd).toHaveBeenCalledTimes(1)
    await act(async () => pending.resolve())
  })
})
