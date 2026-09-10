export function injectionLabel(outcome: string): string {
  if (/^Typed(?: \{ backend: (?:Ydotool|Xdotool|Wtype) \}| · (?:Ydotool|Xdotool|Wtype))?$/.test(outcome)) return 'Inserted by typing'
  if (/^Pasted(?: \{ backend: (?:Ydotool|Xdotool|Wtype) \}| · (?:Ydotool|Xdotool|Wtype))?$/.test(outcome)) return 'Inserted by pasting'
  if (outcome === 'ClipboardOnly') return 'Clipboard fallback. Copy to paste.'
  if (outcome === 'Failed' || /^Failed \{ reason: \w+ \}$/.test(outcome)) return 'Insertion failed. Copy to paste.'
  return 'Insertion outcome unavailable'
}
