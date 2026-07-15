use std::collections::VecDeque;
use std::time::{Duration, SystemTime};

use crate::app::menu_flow::test_support::test_services;
use crate::app::session_terminal::SessionAtTerminalFailure;
use crate::app::terminal::{Terminal, TerminalEcho, TerminalFuture, TerminalRead};
use crate::domain::session::typed::{ActivePhase, NewUserRegisteringSession};
use crate::domain::session::{LogonChannel, Session};

use super::{RegistrationFlow, IDLE_TIMEOUT_LINE};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fault {
    Write,
}

struct FaultTerminal {
    inputs: VecDeque<TerminalRead>,
    fail_write_at: Option<usize>,
    fail_write_containing: Option<Vec<u8>>,
    write_count: usize,
}

impl FaultTerminal {
    fn failing_first_write() -> Self {
        Self {
            inputs: VecDeque::new(),
            fail_write_at: Some(0),
            fail_write_containing: None,
            write_count: 0,
        }
    }

    fn idle_then_fail_timeout_notice() -> Self {
        Self {
            inputs: [TerminalRead::IdleTimedOut].into(),
            fail_write_at: None,
            fail_write_containing: Some(IDLE_TIMEOUT_LINE.to_vec()),
            write_count: 0,
        }
    }
}

impl Terminal for FaultTerminal {
    type Error = Fault;

    fn write<'a>(&'a mut self, bytes: &'a [u8]) -> TerminalFuture<'a, (), Self::Error> {
        Box::pin(async move {
            let operation = self.write_count;
            self.write_count += 1;
            if self.fail_write_at == Some(operation)
                || self.fail_write_containing.as_deref().is_some_and(|needle| {
                    bytes.windows(needle.len()).any(|window| window == needle)
                })
            {
                return Err(Fault::Write);
            }
            Ok(())
        })
    }

    fn flush(&mut self) -> TerminalFuture<'_, (), Self::Error> {
        Box::pin(async { Ok(()) })
    }

    fn read_line(
        &mut self,
        _echo: TerminalEcho,
        _timeout: Duration,
    ) -> TerminalFuture<'_, TerminalRead, Self::Error> {
        Box::pin(async move { Ok(self.inputs.pop_front().unwrap_or(TerminalRead::Eof)) })
    }
}

#[tokio::test]
async fn opening_screen_failure_retains_the_registering_session() {
    let services = test_services();
    let mut terminal = FaultTerminal::failing_first_write();

    let Err(failure) = RegistrationFlow::new(&mut terminal, &services)
        .run(registering_session(), false)
        .await
    else {
        panic!("screen failure must escape with session ownership");
    };

    let (phase, source) = failure.into_parts();
    assert_eq!(source, Fault::Write);
    assert!(matches!(
        phase,
        SessionAtTerminalFailure::Active(ActivePhase::NewUserRegistering(_))
    ));
}

#[tokio::test]
async fn idle_timeout_transitions_before_a_timeout_notice_failure() {
    let services = test_services();
    let mut terminal = FaultTerminal::idle_then_fail_timeout_notice();

    let Err(failure) = RegistrationFlow::new(&mut terminal, &services)
        .run(registering_session(), false)
        .await
    else {
        panic!("timeout notice failure must escape with session ownership");
    };

    let (phase, source) = failure.into_parts();
    assert_eq!(source, Fault::Write);
    assert!(matches!(phase, SessionAtTerminalFailure::LoggingOff(_)));
}

fn registering_session() -> NewUserRegisteringSession {
    let mut session = Session::new(1, LogonChannel::Remote, 9_600, SystemTime::UNIX_EPOCH);
    session.prompt_for_name().expect("identifying");
    session
        .record_new_user_request(true, false, SystemTime::UNIX_EPOCH)
        .expect("registration starts");
    NewUserRegisteringSession::from_session(session)
}
