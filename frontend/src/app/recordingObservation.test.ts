import { describe, expect, it } from 'vitest'
import { richPreviewStatus } from '../api/previewDesktopFixtures'
import type { AppStatus } from '../generated/ipc'
import {
  acceptRecordingObservation,
  advanceRecordingObservationEpoch,
  createRecordingObservationState,
  type RecordingObservation,
} from './recordingObservation'

describe('recording observations', () => {
  it('rejects a poll that began before a command epoch', () => {
    const recording = {
      ...richPreviewStatus(),
      phase: 'Recording',
      recordingSessionId: 'session-a',
      recordingRevision: 2,
    } satisfies AppStatus
    const current = advanceRecordingObservationEpoch(createRecordingObservationState(recording))

    const result = acceptRecordingObservation(current, {
      kind: 'poll',
      epoch: 0,
      snapshot: { ...recording, phase: 'Transcribing', recordingRevision: 4 },
    })

    expect(result).toBe(current)
  })

  it('rejects a delayed acknowledgement after a poll observes progress or session replacement', () => {
    const recording = {
      ...richPreviewStatus(),
      phase: 'Recording',
      recordingSessionId: 'session-a',
      recordingRevision: 2,
    } satisfies AppStatus
    const progressed = {
      ...recording,
      phase: 'Transcribing',
      recordingRevision: 4,
    } satisfies AppStatus
    const replacement = {
      ...progressed,
      recordingSessionId: 'session-b',
    } as const
    const acknowledgement = {
      kind: 'acknowledgement',
      requestedFrom: 'session-a',
      snapshot: {
        sessionId: 'session-a',
        phase: 'Recording',
        captureStopRequested: true,
        revision: 3,
      },
    } satisfies RecordingObservation

    const afterProgress = acceptRecordingObservation(
      createRecordingObservationState(progressed),
      acknowledgement,
    )
    const replacementState = createRecordingObservationState(replacement)
    const afterReplacement = acceptRecordingObservation(replacementState, acknowledgement)

    expect(afterProgress.snapshot).toBe(progressed)
    expect(afterReplacement).toBe(replacementState)
  })

  it('keeps same-session recording revisions ordered', () => {
    const current = {
      ...richPreviewStatus(),
      phase: 'Transcribing',
      recordingSessionId: 'session-a',
      recordingRevision: 4,
    } satisfies AppStatus
    const state = createRecordingObservationState(current)

    const result = acceptRecordingObservation(state, {
      kind: 'poll',
      epoch: 0,
      snapshot: { ...current, phase: 'Recording', recordingRevision: 3 },
    })

    expect(result).toBe(state)
  })
})
