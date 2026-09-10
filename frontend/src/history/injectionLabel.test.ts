import { expect, it } from 'vitest'

import { injectionLabel } from './injectionLabel'

it.each(['Ydotool', 'Xdotool', 'Wtype'])('describes the serialized insertion outcomes for %s', (backend) => {
  expect(injectionLabel(`Typed · ${backend}`)).toBe('Inserted by typing')
  expect(injectionLabel(`Pasted · ${backend}`)).toBe('Inserted by pasting')
  expect(injectionLabel(`Typed { backend: ${backend} }`)).toBe('Inserted by typing')
  expect(injectionLabel(`Pasted { backend: ${backend} }`)).toBe('Inserted by pasting')
})

it('describes legacy, recovery, and failure outcomes without guessing unknown success', () => {
  expect(injectionLabel('Typed')).toBe('Inserted by typing')
  expect(injectionLabel('Pasted')).toBe('Inserted by pasting')
  expect(injectionLabel('ClipboardOnly')).toBe('Clipboard fallback. Copy to paste.')
  expect(injectionLabel('Failed { reason: InjectUnconfirmed }')).toBe('Insertion failed. Copy to paste.')
  for (const unknown of ['', 'TypedMaybe', 'Pasted { backend: Future }', 'unknown']) {
    expect(injectionLabel(unknown)).toBe('Insertion outcome unavailable')
  }
})
