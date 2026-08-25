import { presentSpeechSetup } from './setup'
import type { ComponentStatus, Readiness, SetupPlan } from './types'

function component(
  id: ComponentStatus['id'],
  label: string,
  managed: ComponentStatus['managed'] = { kind: 'absent', resumableBytes: 0 },
): ComponentStatus {
  return {
    id,
    label,
    managed,
    external: [],
    activeOrigin: null,
    activity: null,
  }
}

function plan(id: SetupPlan['id'], satisfied = false): SetupPlan {
  const components: Record<SetupPlan['id'], ComponentStatus['id'][]> = {
    recommended: ['whisper-runtime', 'whisper-small'],
    parakeet: ['sherpa-runtime', 'parakeet-tdt-06b-v3-int8'],
    'whisper-base': ['whisper-runtime', 'whisper-base-q5-1'],
    'whisper-small': ['whisper-runtime', 'whisper-small'],
    'whisper-large-v3-turbo': ['whisper-runtime', 'whisper-large-v3-turbo-q5-0'],
  }
  return {
    id,
    label: id,
    components: components[id],
    satisfied,
    downloadBytes: 100,
    requiredFreeBytes: 200,
    availableBytes: 300,
    diskReady: true,
    diskReason: null,
  }
}

function readiness(overrides: Partial<Readiness> = {}): Readiness {
  return {
    managedSupported: true,
    unsupportedReason: null,
    totalMemoryBytes: null,
    recommendedModel: 'whisper-small',
    components: [component('whisper-runtime', 'Whisper runtime')],
    plans: [plan('recommended'), plan('parakeet'), plan('whisper-base')],
    microphoneReady: true,
    speechReady: false,
    hasSuccessfulDictation: false,
    firstRunComplete: false,
    activeOperation: null,
    activeCancellable: false,
    ...overrides,
  }
}

describe('speech setup presentation', () => {
  it('reduces a ready inventory to one useful summary', () => {
    const runtime = component('whisper-runtime', 'Whisper runtime')
    runtime.activeOrigin = 'system'
    const model = component('whisper-small', 'Small multilingual')
    model.activeOrigin = 'external'
    const presented = presentSpeechSetup(readiness({ speechReady: true, components: [runtime, model] }))

    expect(presented.state).toEqual({
      kind: 'ready',
      title: 'Ready to dictate',
      detail: 'A local speech engine and model are available.',
    })
    expect(presented.installedComponents).toHaveLength(2)
  })

  it('keeps recommended and visible Parakeet out of duplicate advanced rows', () => {
    const source = readiness()
    const presented = presentSpeechSetup(source)

    expect(presented.state.kind).toBe('needs-setup')
    expect(presented.recommended).toBe(source.plans[0])
    expect(presented.parakeet).toBe(source.plans[1])
    expect(presented.alternativePlans.map((candidate) => candidate.id)).toEqual(['whisper-base'])
  })

  it('removes a concrete plan with the same components as Recommended', () => {
    const presented = presentSpeechSetup(readiness({
      speechReady: true,
      plans: [
        plan('recommended'),
        plan('parakeet'),
        plan('whisper-large-v3-turbo'),
        plan('whisper-small'),
        plan('whisper-base'),
      ],
    }))

    expect(presented.alternativePlans.map((candidate) => candidate.id)).toEqual([
      'parakeet',
      'whisper-large-v3-turbo',
      'whisper-base',
    ])
  })

  it('presents real component progress without inventing an overall percentage', () => {
    const runtime = component('whisper-runtime', 'Whisper runtime')
    runtime.activity = {
      operationId: 'op-1',
      component: 'whisper-runtime',
      phase: 'downloading',
      receivedBytes: 25,
      totalBytes: 100,
      resumedFromBytes: 0,
    }
    const presented = presentSpeechSetup(
      readiness({ components: [runtime], activeOperation: 'op-1', activeCancellable: true }),
    )

    expect(presented.state).toEqual({
      kind: 'in-progress',
      title: 'Setting up speech',
      detail: 'Whisper runtime · 25% downloaded',
      component: runtime,
    })
  })

  it('uses neutral maintenance copy when an operation has no setup progress', () => {
    const presented = presentSpeechSetup(
      readiness({ activeOperation: 'verify-runtime', activeCancellable: false }),
    )

    expect(presented.state).toEqual({
      kind: 'in-progress',
      title: 'Speech maintenance in progress',
      detail: 'Please wait for this operation to finish.',
      component: null,
    })
  })

  it('keeps unsupported and repair errors in the summary state', () => {
    expect(
      presentSpeechSetup(
        readiness({ managedSupported: false, unsupportedReason: 'Linux package unavailable' }),
      ).state,
    ).toMatchObject({ kind: 'unsupported', detail: 'Linux package unavailable' })

    const damaged = component('whisper-runtime', 'Whisper runtime', {
      kind: 'needs-repair',
      reason: 'checksum mismatch',
      resumableBytes: 0,
    })
    expect(presentSpeechSetup(readiness({ components: [damaged] })).state).toMatchObject({
      kind: 'needs-repair',
      component: damaged,
    })
  })
})
