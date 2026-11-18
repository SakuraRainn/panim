//! The input in the browser windows, for the automatic lock, without Tauri. Refer to `docs/ARCHITECTURE.md` §2.
//!
//! - On Linux, the app gets the key, button and scroll events of each browser window from GTK, before WebKit gets
//!   them. The script of a page cannot make these events.
//! - [`Throttle`] lets the app send no more than one event `input-elsewhere` to the main window in [`GAP`].

use std::time::{Duration, Instant};

/// The minimum time between two events `input-elsewhere`.
pub const GAP: Duration = Duration::from_secs(10);

/// Lets through no more than one input in [`GAP`].
#[derive(Debug, Default)]
pub struct Throttle {
    /// The time of the last input that the throttle let through. `None` before the first input.
    last: Option<Instant>,
}

impl Throttle {
    /// Makes a throttle that lets through the first input.
    pub const fn new() -> Self {
        Self { last: None }
    }

    /// Records an input at `now`. Returns true if the app must send the event. Returns false if
    /// the last event was less than [`GAP`] before.
    pub fn input(&mut self, now: Instant) -> bool {
        match self.last {
            Some(last) if now.saturating_duration_since(last) < GAP => false,
            _ => {
                self.last = Some(now);
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_input_passes() {
        let mut throttle = Throttle::new();
        assert!(throttle.input(Instant::now()));
    }

    #[test]
    fn input_in_the_gap_does_not_pass() {
        let start = Instant::now();
        let mut throttle = Throttle::new();
        assert!(throttle.input(start));
        assert!(!throttle.input(start));
        assert!(!throttle.input(start + Duration::from_secs(1)));
        assert!(!throttle.input(start + GAP - Duration::from_millis(1)));
    }

    #[test]
    fn input_after_the_gap_passes() {
        let start = Instant::now();
        let mut throttle = Throttle::new();
        assert!(throttle.input(start));
        assert!(throttle.input(start + GAP));
        assert!(!throttle.input(start + GAP + Duration::from_secs(9)));
        assert!(throttle.input(start + GAP * 2));
    }

    #[test]
    fn blocked_input_does_not_move_the_gap() {
        // Input without a stop gives one event each 10 seconds, not only one event at the start.
        let start = Instant::now();
        let mut throttle = Throttle::new();
        let passed = (0..=30)
            .filter(|s| throttle.input(start + Duration::from_secs(*s)))
            .count();
        assert_eq!(passed, 4);
    }

    #[test]
    fn earlier_time_does_not_pass() {
        // A clock that goes back does not let an input through.
        let start = Instant::now() + Duration::from_secs(60);
        let mut throttle = Throttle::new();
        assert!(throttle.input(start));
        assert!(!throttle.input(start - Duration::from_secs(5)));
    }
}
