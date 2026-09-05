import type { AppStatus, RecordingSnapshot } from '../generated/ipc'

export type RecordingObservationState = Readonly<{
  snapshot: AppStatus
  epoch: number
}>

export type RecordingObservation =
  | Readonly<{
      kind: 'poll'
      snapshot: AppStatus
      epoch: number
    }>
  | Readonly<{
      kind: 'acknowledgement'
      snapshot: RecordingSnapshot
      requestedFrom: string | null
    }>

export function createRecordingObservationState(snapshot: AppStatus): RecordingObservationState {
  return { snapshot, epoch: 0 }
}

export function advanceRecordingObservationEpoch(
  state: RecordingObservationState,
): RecordingObservationState {
  return { ...state, epoch: state.epoch + 1 }
}

export function acceptRecordingObservation(
  state: RecordingObservationState,
  observation: RecordingObservation,
): RecordingObservationState {
  switch (observation.kind) {
    case 'poll':
      if (observation.epoch !== state.epoch) return state
      return acceptSnapshot(state, observation.snapshot)
    case 'acknowledgement':
      return acceptAcknowledgement(state, observation)
    default: {
      const _exhaustive: never = observation
      return _exhaustive
    }
  }
}

function acceptAcknowledgement(
  state: RecordingObservationState,
  observation: Extract<RecordingObservation, { kind: 'acknowledgement' }>,
): RecordingObservationState {
  const current = state.snapshot
  const { snapshot } = observation
  if (
    current.recordingSessionId !== observation.requestedFrom &&
    current.recordingSessionId !== snapshot.sessionId
  ) return state

  return acceptSnapshot(state, {
    ...current,
    phase: snapshot.phase,
    recordingSessionId: snapshot.sessionId,
    captureStopRequested: snapshot.captureStopRequested,
    recordingRevision: snapshot.revision,
  })
}

function acceptSnapshot(
  state: RecordingObservationState,
  snapshot: AppStatus,
): RecordingObservationState {
  const previous = state.snapshot
  if (
    snapshot.recordingSessionId !== null &&
    snapshot.recordingSessionId === previous.recordingSessionId &&
    snapshot.recordingRevision < previous.recordingRevision
  ) return state

  return { ...state, snapshot }
}
