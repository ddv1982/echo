import { presentShortcut } from './shortcut'
import type { GnomeShortcutSetup } from './types'

describe('shortcut presenter', () => {
  it('uses the portal effective trigger in the stable verification identity', () => {
    const presented = presentShortcut({
      kind: 'active',
      desired: 'Super+Alt+Space',
      effective: 'Alt+F8',
      backend: 'portal',
      activation: 'portal:42',
      verificationIdentity: 'portal:Alt+F8',
    })
    expect(presented.display).toBe('Alt+F8')
    expect(presented.desired).toBe('Super+Alt+Space')
    expect(presented.activation).toBe('portal:42')
    expect(presented.verificationIdentity).toBe('portal:Alt+F8')
  })

  it.each([
    ['missing', 'GNOME setup required'],
    ['stale', 'GNOME repair required'],
    ['conflicting', 'Resolve the GNOME conflict'],
    ['unsupported', 'GNOME shortcut unavailable'],
  ] satisfies Array<[GnomeShortcutSetup['state'], string]>) (
    'presents the %s GNOME state',
    (state, label) => {
      const presented = presentShortcut({
        kind: 'gnome-setup',
        desired: 'Super+Alt+Space',
        setup: {
          state,
          detail: 'GNOME detail',
          command: '/usr/bin/echo-desktop rec --toggle',
          binding: '<Super><Alt>space',
        },
      })
      expect(presented.statusLabel).toBe(label)
      expect(presented.testable).toBe(false)
    },
  )

  it('shows manual setup without claiming or verifying an effective binding', () => {
    const presented = presentShortcut({
      kind: 'manual',
      desired: 'Super+Alt+Space',
      detail: 'Add this binding in compositor settings.',
      command: '/usr/bin/echo-desktop rec --toggle',
    })
    expect(presented.testable).toBe(false)
    expect(presented.ready).toBe(false)
    expect(presented.verificationIdentity).toBeNull()
    expect(presented.manualCommand).toBe('/usr/bin/echo-desktop rec --toggle')
    expect(presented.statusLabel).toBe('Manual shortcut setup')
  })

  it('presents a ready GNOME binding as ready everywhere', () => {
    const presented = presentShortcut({
      kind: 'gnome-ready',
      desired: 'Super+Alt+Space',
      effective: 'Super+Alt+Space',
      detail: 'GNOME owns this Echo shortcut.',
      command: '/usr/bin/echo-desktop rec --toggle',
      binding: '<Super><Alt>space',
      activation: null,
      verificationIdentity: 'gnome:<Super><Alt>space:/usr/bin/echo-desktop rec --toggle',
    })
    expect(presented.ready).toBe(true)
    expect(presented.expectedActivationSource).toBe('toggle-command')
  })

  it('does not offer retry for an unsupported desktop session', () => {
    const presented = presentShortcut({
      kind: 'unsupported',
      desired: 'Super+Alt+Space',
      detail: 'Unknown or headless desktop session.',
    })
    expect(presented.canRetry).toBe(false)
  })
})
