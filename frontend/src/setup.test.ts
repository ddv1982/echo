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
  return {
    id,
    label: id,
    components: [],
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
      detail: 'Whisper runtime · Small multilingual',
    })
    expect(presented.installedComponents).toHaveLength(2)
  })

  it('keeps recommended and Parakeet available without copying backend plans', () => {
    const source = readiness()
    const presented = presentSpeechSetup(source)

    expect(presented.state.kind).toBe('needs-setup')
    expect(presented.recommended).toBe(source.plans[0])
    expect(presented.parakeet).toBe(source.plans[1])
    expect(presented.alternativePlans.map((candidate) => candidate.id)).toEqual([
      'parakeet',
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
