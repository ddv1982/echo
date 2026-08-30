import type { GnomeShortcutSetup, ShortcutStatus } from './generated/ipc'

export interface ShortcutPresentation {
  desired: string
  display: string
  ready: boolean
  testable: boolean
  description: string
  statusLabel: string
  tone: 'ok' | 'attention'
  activation: string | null
  expectedActivationSource: 'native-toggle' | 'toggle-command' | null
  verificationIdentity: string | null
  manualCommand: string | null
  gnomeSetup: GnomeShortcutSetup | null
  canRetry: boolean
}

export function presentShortcut(status: ShortcutStatus): ShortcutPresentation {
  switch (status.kind) {
    case 'probing':
      return base(status.desired, 'Checking desktop shortcut support…')
    case 'active':
      return {
        ...base(
          status.desired,
          `Active through ${status.backend === 'x11' ? 'X11' : 'the desktop portal'}.`,
        ),
        display: status.effective,
        ready: true,
        testable: true,
        statusLabel: status.backend === 'x11' ? 'X11 shortcut active' : 'Desktop shortcut active',
        tone: 'ok',
        activation: status.activation,
        expectedActivationSource: 'native-toggle',
        verificationIdentity: status.verificationIdentity,
      }
    case 'gnome-ready':
      return {
        ...base(status.desired, status.detail),
        display: status.effective,
        ready: true,
        testable: true,
        statusLabel: 'GNOME shortcut ready',
        tone: 'ok',
        activation: status.activation,
        expectedActivationSource: 'toggle-command',
        verificationIdentity: status.verificationIdentity,
      }
    case 'gnome-setup':
      return presentGnomeSetup(status.desired, status.setup)
    case 'manual':
      return {
        ...base(status.desired, status.detail),
        statusLabel: 'Manual shortcut setup',
        manualCommand: status.command,
      }
    case 'failed':
      return { ...base(status.desired, status.detail), statusLabel: 'Shortcut unavailable', canRetry: true }
    case 'unsupported':
      return { ...base(status.desired, status.detail), statusLabel: 'Shortcut unsupported' }
    default: {
      const exhaustive: never = status
      return exhaustive
    }
  }
}

function presentGnomeSetup(
  desired: string,
  setup: GnomeShortcutSetup,
): ShortcutPresentation {
  const shared = { ...base(desired, setup.detail), gnomeSetup: setup }
  switch (setup.state) {
    case 'missing':
      return { ...shared, statusLabel: 'GNOME setup required' }
    case 'stale':
      return { ...shared, statusLabel: 'GNOME repair required' }
    case 'conflicting':
      return { ...shared, statusLabel: 'Resolve the GNOME conflict' }
    case 'unsupported':
      return { ...shared, statusLabel: 'GNOME shortcut unavailable' }
    default: {
      const exhaustive: never = setup.state
      return exhaustive
    }
  }
}

function base(desired: string, description: string): ShortcutPresentation {
  return {
    desired,
    display: desired,
    ready: false,
    testable: false,
    description,
    statusLabel: 'Checking shortcut',
    tone: 'attention',
    activation: null,
    expectedActivationSource: null,
    verificationIdentity: null,
    manualCommand: null,
    gnomeSetup: null,
    canRetry: false,
  }
}
