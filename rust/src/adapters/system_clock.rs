//! Clock adapters: the production system clock and a manually driven
//! clock for tests.

use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use crate::app::clock::Clock;

/// Production [`Clock`]: delegates to [`SystemTime::now`].
#[derive(Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}

/// Manually driven [`Clock`] for tests: reports a fixed instant until
/// [`set`](ManualClock::set) or [`advance`](ManualClock::advance)
/// moves it. Lets a test pin an exact rendered timestamp or step a
/// session across a date boundary.
#[derive(Debug)]
pub struct ManualClock {
    now: Mutex<SystemTime>,
}

impl ManualClock {
    /// A clock frozen at `instant`.
    #[must_use]
    pub fn set_to(instant: SystemTime) -> Self {
        Self {
            now: Mutex::new(instant),
        }
    }

    /// Moves the clock to `instant`.
    ///
    /// # Panics
    /// Panics if the clock mutex is poisoned (a prior holder panicked).
    pub fn set(&self, instant: SystemTime) {
        *self.now.lock().expect("manual clock mutex") = instant;
    }

    /// Advances the clock by `delta`.
    ///
    /// # Panics
    /// Panics if the clock mutex is poisoned (a prior holder panicked).
    pub fn advance(&self, delta: Duration) {
        let mut now = self.now.lock().expect("manual clock mutex");
        *now += delta;
    }
}

impl Clock for ManualClock {
    fn now(&self) -> SystemTime {
        *self.now.lock().expect("manual clock mutex")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_clock_reports_set_then_advanced_instants() {
        let epoch = SystemTime::UNIX_EPOCH;
        let clock = ManualClock::set_to(epoch);
        assert_eq!(clock.now(), epoch);
        clock.advance(Duration::from_mins(1));
        assert_eq!(clock.now(), epoch + Duration::from_mins(1));
        clock.set(epoch + Duration::from_secs(5));
        assert_eq!(clock.now(), epoch + Duration::from_secs(5));
    }
}
