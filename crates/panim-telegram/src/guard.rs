//! The guard: a second protection for a proxy route.
//!
//! TDLib without the patch of PANIM goes past the proxy when the state is `connectionStateConnecting` for
//! 5 seconds. The patch (`nix/tdlib-no-direct.patch`) removes that code. The guard is for a build that does not
//! have the patch: it stops the network of TDLib before the 5 seconds pass, and starts it again after a pause.
//!
//! The guard is a small state machine. The time is a parameter, so the tests need no clock.
//! The guard does not run for a direct route.

use std::time::{Duration, Instant};

/// The guard stops the network after this time in the state "Connecting". TDLib acts after 5 seconds.
pub const CONNECTING_LIMIT: Duration = Duration::from_millis(4000);

/// The network stays off for this time.
pub const PAUSE: Duration = Duration::from_millis(2000);

/// What the client must do now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuardAction {
    Nothing,
    /// Send `setNetworkType` with `networkTypeNone`.
    NetworkOff,
    /// Send `setNetworkType` with `networkTypeOther`.
    NetworkOn,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    /// The state of TDLib is not "Connecting".
    Idle,
    /// The state of TDLib is "Connecting" from this time.
    Connecting(Instant),
    /// The guard stopped the network at this time.
    Paused(Instant),
}

pub struct Guard {
    enabled: bool,
    state: State,
}

impl Guard {
    /// `enabled` is true only for a proxy route.
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            state: State::Idle,
        }
    }

    /// Records a connection state of TDLib. `connecting` is true only for `connectionStateConnecting`.
    pub fn on_state(&mut self, connecting: bool, now: Instant) {
        if !self.enabled {
            return;
        }
        self.state = match (self.state, connecting) {
            // The pause continues until its time ends. TDLib tells "waiting for network" during the pause.
            (State::Paused(since), _) => State::Paused(since),
            (State::Connecting(since), true) => State::Connecting(since),
            (State::Idle, true) => State::Connecting(now),
            (_, false) => State::Idle,
        };
    }

    /// Tells what to do at this time. Call it frequently, for example four times each second.
    pub fn tick(&mut self, now: Instant) -> GuardAction {
        if !self.enabled {
            return GuardAction::Nothing;
        }
        match self.state {
            State::Connecting(since) if now.duration_since(since) >= CONNECTING_LIMIT => {
                self.state = State::Paused(now);
                GuardAction::NetworkOff
            }
            State::Paused(since) if now.duration_since(since) >= PAUSE => {
                // TDLib tells its new state after the network is on. Until then, the guard waits.
                self.state = State::Idle;
                GuardAction::NetworkOn
            }
            _ => GuardAction::Nothing,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(start: Instant, millis: u64) -> Instant {
        start + Duration::from_millis(millis)
    }

    #[test]
    fn direct_route_has_no_guard() {
        let start = Instant::now();
        let mut guard = Guard::new(false);
        guard.on_state(true, start);
        for millis in [0, 4000, 10_000, 60_000] {
            assert_eq!(guard.tick(ms(start, millis)), GuardAction::Nothing);
        }
    }

    #[test]
    fn network_stops_before_five_seconds_of_connecting() {
        let start = Instant::now();
        let mut guard = Guard::new(true);
        guard.on_state(true, start);
        assert_eq!(guard.tick(ms(start, 3999)), GuardAction::Nothing);
        assert_eq!(guard.tick(ms(start, 4000)), GuardAction::NetworkOff);
        assert!(CONNECTING_LIMIT < Duration::from_secs(5));
        // TDLib tells "waiting for network". The pause continues.
        guard.on_state(false, ms(start, 4100));
        assert_eq!(guard.tick(ms(start, 5999)), GuardAction::Nothing);
        assert_eq!(guard.tick(ms(start, 6000)), GuardAction::NetworkOn);
        // The network is on again, and TDLib is "Connecting" again. A new period starts.
        guard.on_state(true, ms(start, 6100));
        assert_eq!(guard.tick(ms(start, 10_000)), GuardAction::Nothing);
        assert_eq!(guard.tick(ms(start, 10_100)), GuardAction::NetworkOff);
    }

    #[test]
    fn a_connection_that_becomes_ready_needs_no_action() {
        let start = Instant::now();
        let mut guard = Guard::new(true);
        guard.on_state(true, start);
        guard.on_state(false, ms(start, 1500));
        assert_eq!(guard.tick(ms(start, 9000)), GuardAction::Nothing);
        // A later period of "Connecting" starts its own time.
        guard.on_state(true, ms(start, 9000));
        assert_eq!(guard.tick(ms(start, 12_999)), GuardAction::Nothing);
        assert_eq!(guard.tick(ms(start, 13_000)), GuardAction::NetworkOff);
    }

    #[test]
    fn a_second_connecting_update_does_not_start_the_time_again() {
        let start = Instant::now();
        let mut guard = Guard::new(true);
        guard.on_state(true, start);
        guard.on_state(true, ms(start, 3000));
        assert_eq!(guard.tick(ms(start, 4000)), GuardAction::NetworkOff);
    }
}
