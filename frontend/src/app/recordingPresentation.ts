import type { AppStatus } from '../generated/ipc'

export function isCanceledRecording(status: Pick<AppStatus, 'phase' | 'lastError'>): boolean {
  return status.phase === 'Failed' && status.lastError === 'Transcription canceled'
}
