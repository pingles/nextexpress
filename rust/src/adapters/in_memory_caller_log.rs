//! In-memory [`CallerLogAppender`] used by tests and the default
//! supervisor. A real adapter that writes to disk lands later.

use std::sync::Mutex;

use crate::domain::caller_log::{CallerLog, CallerLogAppender};

/// In-memory adapter that buffers [`CallerLog`] entries in a
/// [`Mutex`]-guarded [`Vec`].
#[derive(Debug, Default)]
pub struct InMemoryCallerLog {
    entries: Mutex<Vec<CallerLog>>,
}

impl InMemoryCallerLog {
    /// Constructs an empty log.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns a snapshot of the entries logged so far.
    pub fn entries(&self) -> Vec<CallerLog> {
        self.entries.lock().expect("caller log mutex").clone()
    }
}

impl CallerLogAppender for InMemoryCallerLog {
    fn append(&self, entry: CallerLog) {
        self.entries.lock().expect("caller log mutex").push(entry);
    }
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use super::*;

    #[test]
    fn append_then_snapshot_returns_entries_in_order() {
        let log = InMemoryCallerLog::new();
        log.append(CallerLog {
            session_node: 1,
            at: SystemTime::UNIX_EPOCH,
            text: "first".to_string(),
            is_password_failure: false,
        });
        log.append(CallerLog {
            session_node: 1,
            at: SystemTime::UNIX_EPOCH,
            text: "second".to_string(),
            is_password_failure: true,
        });
        let entries = log.entries();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].text, "first");
        assert_eq!(entries[1].text, "second");
        assert!(!entries[0].is_password_failure);
        assert!(entries[1].is_password_failure);
    }
}
