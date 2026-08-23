import type { ComponentStatus, Readiness, SetupPlan } from './types'

export type SpeechSetupState =
  | { kind: 'ready'; title: 'Ready to dictate'; detail: string }
  | { kind: 'needs-setup'; title: 'Speech setup needed'; detail: string }
  | { kind: 'in-progress'; title: 'Setting up speech'; detail: string; component: ComponentStatus | null }
  | { kind: 'needs-repair'; title: 'Speech setup needs attention'; detail: string; component: ComponentStatus }
  | { kind: 'unsupported'; title: 'Managed setup unavailable'; detail: string }

export interface SpeechSetupPresentation {
  state: SpeechSetupState
  recommended: SetupPlan | null
  parakeet: SetupPlan | null
  installedComponents: ComponentStatus[]
  alternativePlans: SetupPlan[]
}

export function presentSpeechSetup(readiness: Readiness): SpeechSetupPresentation {
  const recommended = readiness.plans.find((plan) => plan.id === 'recommended') ?? null
  const parakeet = readiness.plans.find((plan) => plan.id === 'parakeet') ?? null
  const active = readiness.components.find((component) => component.activity != null) ?? null
  const damaged = readiness.components.find((component) => component.managed.kind === 'needs-repair')
  let state: SpeechSetupState

  if (readiness.activeOperation != null) {
    state = {
      kind: 'in-progress',
      title: 'Setting up speech',
      detail: active == null ? 'Preparing the next component' : setupActivityLabel(active),
      component: active,
    }
  } else if (!readiness.managedSupported && !readiness.speechReady) {
    state = {
      kind: 'unsupported',
      title: 'Managed setup unavailable',
      detail: readiness.unsupportedReason ?? 'Install a supported runtime and model manually.',
    }
  } else if (damaged != null && !readiness.speechReady) {
    state = {
      kind: 'needs-repair',
      title: 'Speech setup needs attention',
      detail: `${damaged.label} needs repair.`,
      component: damaged,
    }
  } else if (readiness.speechReady) {
    state = {
      kind: 'ready',
      title: 'Ready to dictate',
      detail: activeSpeechDetail(readiness.components),
    }
  } else {
    state = {
      kind: 'needs-setup',
      title: 'Speech setup needed',
      detail: recommended?.diskReason ?? 'Install the recommended local speech engine and model.',
    }
  }

  return {
    state,
    recommended,
    parakeet,
    installedComponents: readiness.components,
    alternativePlans: readiness.plans.filter((plan) => plan.id !== 'recommended'),
  }
}

function setupActivityLabel(component: ComponentStatus): string {
  const activity = component.activity
  if (activity == null) return `Preparing ${component.label}`
  if (activity.phase === 'downloading' && activity.totalBytes > 0) {
    return `${component.label} · ${Math.floor((activity.receivedBytes / activity.totalBytes) * 100)}% downloaded`
  }
  return `${component.label} · ${activity.phase.replace('-', ' ')}`
}

function activeSpeechDetail(components: ComponentStatus[]): string {
  const active = components.filter((component) => component.activeOrigin != null)
  if (active.length === 0) return 'Your local speech engine and model are available.'
  return active.map((component) => component.label).join(' · ')
}
