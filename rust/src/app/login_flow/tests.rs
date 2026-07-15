use std::collections::VecDeque;
use std::time::{Duration, SystemTime};

use crate::app::menu_flow::test_support::test_services;
use crate::app::session_terminal::SessionAtTerminalFailure;
use crate::app::terminal::{Terminal, TerminalEcho, TerminalFuture, TerminalRead};
use crate::domain::session::typed::{ActivePhase, ConnectingSession};
use crate::domain::session::LogonChannel;

use super::{LoginFlow, IDLE_TIMEOUT_LINE, TOO_MANY_RETRIES_LINE};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fault {
    Read,
    Write,
    Flush,
}

struct FaultTerminal {
    inputs: VecDeque<TerminalRead>,
    fail_read: bool,
    fail_flush: bool,
    fail_write_containing: Option<Vec<u8>>,
}

impl FaultTerminal {
    fn failing_read() -> Self {
        Self {
            inputs: VecDeque::new(),
            fail_read: true,
            fail_flush: false,
            fail_write_containing: None,
        }
    }

    fn failing_flush() -> Self {
        Self {
            inputs: VecDeque::new(),
            fail_read: false,
            fail_flush: true,
            fail_write_containing: None,
        }
    }

    fn with_inputs_and_failing_write(
        inputs: impl IntoIterator<Item = TerminalRead>,
        needle: &[u8],
    ) -> Self {
        Self {
            inputs: inputs.into_iter().collect(),
            fail_read: false,
            fail_flush: false,
            fail_write_containing: Some(needle.to_vec()),
        }
    }
}

impl Terminal for FaultTerminal {
    type Error = Fault;

    fn write<'a>(&'a mut self, bytes: &'a [u8]) -> TerminalFuture<'a, (), Self::Error> {
        Box::pin(async move {
            if self
                .fail_write_containing
                .as_deref()
                .is_some_and(|needle| bytes.windows(needle.len()).any(|window| window == needle))
            {
                return Err(Fault::Write);
            }
            Ok(())
        })
    }

    fn flush(&mut self) -> TerminalFuture<'_, (), Self::Error> {
        Box::pin(async move {
            if self.fail_flush {
                return Err(Fault::Flush);
            }
            Ok(())
        })
    }

    fn read_line(
        &mut self,
        _echo: TerminalEcho,
        _timeout: Duration,
    ) -> TerminalFuture<'_, TerminalRead, Self::Error> {
        Box::pin(async move {
            if self.fail_read {
                return Err(Fault::Read);
            }
            Ok(self.inputs.pop_front().unwrap_or(TerminalRead::Eof))
        })
    }
}

#[tokio::test]
async fn read_failure_retains_the_identifying_session() {
    let services = test_services();
    let mut terminal = FaultTerminal::failing_read();
    let session = identifying_session();

    let Err(failure) = LoginFlow::new(&mut terminal, &services)
        .identify(session)
        .await
    else {
        panic!("read failure must escape with session ownership");
    };

    let (phase, source) = failure.into_parts();
    assert_eq!(source, Fault::Read);
    assert!(matches!(
        phase,
        SessionAtTerminalFailure::Active(ActivePhase::Identifying(_))
    ));
}

#[tokio::test]
async fn prompt_flush_failure_retains_the_identifying_session() {
    let services = test_services();
    let mut terminal = FaultTerminal::failing_flush();

    let Err(failure) = LoginFlow::new(&mut terminal, &services)
        .identify(identifying_session())
        .await
    else {
        panic!("flush failure must escape with session ownership");
    };

    let (phase, source) = failure.into_parts();
    assert_eq!(source, Fault::Flush);
    assert!(matches!(
        phase,
        SessionAtTerminalFailure::Active(ActivePhase::Identifying(_))
    ));
}

#[tokio::test]
async fn idle_timeout_transitions_before_a_timeout_notice_failure() {
    let services = test_services();
    let mut terminal = FaultTerminal::with_inputs_and_failing_write(
        [TerminalRead::IdleTimedOut],
        IDLE_TIMEOUT_LINE,
    );

    let Err(failure) = LoginFlow::new(&mut terminal, &services)
        .identify(identifying_session())
        .await
    else {
        panic!("timeout notice failure must escape with session ownership");
    };

    let (phase, source) = failure.into_parts();
    assert_eq!(source, Fault::Write);
    assert!(matches!(phase, SessionAtTerminalFailure::LoggingOff(_)));
}

#[tokio::test]
async fn ended_session_is_retained_when_the_retry_notice_fails() {
    let services = test_services();
    let mut inputs = vec![TerminalRead::Line("Y".to_string())];
    inputs.extend((0..5).map(|_| TerminalRead::Line("nobody".to_string())));
    let mut terminal = FaultTerminal::with_inputs_and_failing_write(inputs, TOO_MANY_RETRIES_LINE);

    let Err(failure) = LoginFlow::new(&mut terminal, &services)
        .identify(identifying_session())
        .await
    else {
        panic!("retry notice failure must retain the ended session");
    };

    let (phase, source) = failure.into_parts();
    assert_eq!(source, Fault::Write);
    assert!(matches!(phase, SessionAtTerminalFailure::Ended(_)));
}

fn identifying_session() -> crate::domain::session::typed::IdentifyingSession {
    ConnectingSession::accept(1, LogonChannel::Remote, 9_600, SystemTime::UNIX_EPOCH)
        .expect("fresh session")
        .prompt_for_name()
}
