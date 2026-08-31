use std::time::Instant;

use crate::types::FailReason;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Idle,
    Recording { started: Instant },
    Transcribing,
    Injecting,
    Failed { reason: FailReason },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionError {
    IllegalTransition {
        from: SessionState,
        event: &'static str,
    },
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IllegalTransition { from, event } => {
                write!(f, "illegal session transition: {event} from {from:?}")
            }
        }
    }
}

impl std::error::Error for SessionError {}

/// One dictation utterance. Platform modules do not own this state.
#[derive(Debug, Clone)]
pub struct Session {
    state: SessionState,
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

impl Session {
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: SessionState::Idle,
        }
    }

    #[must_use]
    pub fn state(&self) -> SessionState {
        self.state
    }

    pub fn start_recording(&mut self) -> Result<(), SessionError> {
        match self.state {
            SessionState::Idle => {
                self.state = SessionState::Recording {
                    started: Instant::now(),
                };
                Ok(())
            }
            _ => Err(self.illegal("start_recording")),
        }
    }

    pub fn finish_recording(&mut self) -> Result<(), SessionError> {
        match self.state {
            SessionState::Recording { .. } => {
                self.state = SessionState::Transcribing;
                Ok(())
            }
            _ => Err(self.illegal("finish_recording")),
        }
    }

    pub fn begin_injecting(&mut self) -> Result<(), SessionError> {
        match self.state {
            SessionState::Transcribing => {
                self.state = SessionState::Injecting;
                Ok(())
            }
            _ => Err(self.illegal("begin_injecting")),
        }
    }

    pub fn complete_inject(&mut self) -> Result<(), SessionError> {
        match self.state {
            SessionState::Injecting => {
                self.state = SessionState::Idle;
                Ok(())
            }
            _ => Err(self.illegal("complete_inject")),
        }
    }

    pub fn fail(&mut self, reason: FailReason) -> Result<(), SessionError> {
        match self.state {
            SessionState::Idle | SessionState::Failed { .. } => Err(self.illegal("fail")),
            _ => {
                self.state = SessionState::Failed { reason };
                Ok(())
            }
        }
    }

    /// Leave `Failed` for `Idle`. This is the only path back after a failed inject.
    pub fn ack(&mut self) -> Result<(), SessionError> {
        match self.state {
            SessionState::Failed { .. } => {
                self.state = SessionState::Idle;
                Ok(())
            }
            _ => Err(self.illegal("ack")),
        }
    }

    fn illegal(&self, event: &'static str) -> SessionError {
        SessionError::IllegalTransition {
            from: self.state,
            event,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Kind {
        Idle,
        Recording,
        Transcribing,
        Injecting,
        Failed,
    }

    #[derive(Clone, Copy, Debug)]
    enum Event {
        StartRecording,
        FinishRecording,
        BeginInjecting,
        CompleteInject,
        Fail,
        Ack,
    }

    fn drive(kind: Kind) -> Session {
        let mut session = Session::new();
        match kind {
            Kind::Idle => {}
            Kind::Recording => session.start_recording().unwrap(),
            Kind::Transcribing => {
                session.start_recording().unwrap();
                session.finish_recording().unwrap();
            }
            Kind::Injecting => {
                session.start_recording().unwrap();
                session.finish_recording().unwrap();
                session.begin_injecting().unwrap();
            }
            Kind::Failed => {
                session.start_recording().unwrap();
                session.fail(FailReason::InjectUnconfirmed).unwrap();
            }
        }
        session
    }

    fn apply(session: &mut Session, event: Event) -> Result<(), SessionError> {
        match event {
            Event::StartRecording => session.start_recording(),
            Event::FinishRecording => session.finish_recording(),
            Event::BeginInjecting => session.begin_injecting(),
            Event::CompleteInject => session.complete_inject(),
            Event::Fail => session.fail(FailReason::EngineError),
            Event::Ack => session.ack(),
        }
    }

    fn classify(state: SessionState) -> Kind {
        match state {
            SessionState::Idle => Kind::Idle,
            SessionState::Recording { .. } => Kind::Recording,
            SessionState::Transcribing => Kind::Transcribing,
            SessionState::Injecting => Kind::Injecting,
            SessionState::Failed { .. } => Kind::Failed,
        }
    }

    fn expected(from: Kind, event: Event) -> Option<Kind> {
        match (from, event) {
            (Kind::Idle, Event::StartRecording) => Some(Kind::Recording),
            (Kind::Recording, Event::FinishRecording) => Some(Kind::Transcribing),
            (Kind::Transcribing, Event::BeginInjecting) => Some(Kind::Injecting),
            (Kind::Injecting, Event::CompleteInject) => Some(Kind::Idle),
            (Kind::Recording | Kind::Transcribing | Kind::Injecting, Event::Fail) => {
                Some(Kind::Failed)
            }
            (Kind::Failed, Event::Ack) => Some(Kind::Idle),
            _ => None,
        }
    }

    #[test]
    fn every_state_event_pair() {
        let kinds = [
            Kind::Idle,
            Kind::Recording,
            Kind::Transcribing,
            Kind::Injecting,
            Kind::Failed,
        ];
        let events = [
            Event::StartRecording,
            Event::FinishRecording,
            Event::BeginInjecting,
            Event::CompleteInject,
            Event::Fail,
            Event::Ack,
        ];
        for from in kinds {
            for event in events {
                let mut session = drive(from);
                let result = apply(&mut session, event);
                match expected(from, event) {
                    Some(to) => {
                        assert!(
                            result.is_ok(),
                            "{from:?} + {event:?} should be legal, got {result:?}"
                        );
                        assert_eq!(
                            classify(session.state()),
                            to,
                            "{from:?} + {event:?} landed in {:?}",
                            session.state()
                        );
                    }
                    None => {
                        assert!(result.is_err(), "{from:?} + {event:?} should be illegal");
                        assert_eq!(
                            classify(session.state()),
                            from,
                            "illegal {event:?} mutated {:?}",
                            session.state()
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn hold_twice_does_not_nest() {
        let mut session = Session::new();
        session.start_recording().unwrap();
        let first = session.state();
        assert!(session.start_recording().is_err());
        assert_eq!(session.state(), first);
    }

    #[test]
    fn failed_inject_returns_to_idle_only_via_ack() {
        let mut session = drive(Kind::Injecting);
        session.fail(FailReason::InjectUnconfirmed).unwrap();
        assert!(matches!(
            session.state(),
            SessionState::Failed {
                reason: FailReason::InjectUnconfirmed
            }
        ));
        assert!(session.complete_inject().is_err());
        assert!(session.start_recording().is_err());
        session.ack().unwrap();
        assert_eq!(session.state(), SessionState::Idle);
    }
}
