//! A request budget: at most `per_minute` requests in any 60 s, and `min_gap` between two.
//! MAL documents no limit, so Kotora budgets 20 a minute, 1 s apart.
//! Pure: the caller passes the clock and does the sleeping, so tests need no real time.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

pub struct Limiter {
    per_minute: usize,
    min_gap: Duration,
    sent: VecDeque<Instant>,
}

impl Limiter {
    pub fn new(per_minute: usize, min_gap: Duration) -> Self {
        Limiter { per_minute: per_minute.max(1), min_gap, sent: VecDeque::new() }
    }

    /// MAL's budget.
    pub fn mal() -> Self {
        Limiter::new(20, Duration::from_secs(1))
    }

    /// How long to wait from `now` before sending. Records the request as sent at that time,
    /// so call it once per request and then sleep that long.
    pub fn acquire(&mut self, now: Instant) -> Duration {
        let mut at = now;
        if let Some(&last) = self.sent.back() {
            at = at.max(last + self.min_gap);
        }
        if self.sent.len() >= self.per_minute {
            // The request `per_minute` places back must be a full minute old.
            let oldest = self.sent[self.sent.len() - self.per_minute];
            at = at.max(oldest + Duration::from_secs(60));
        }
        self.sent.push_back(at);
        while self.sent.len() > self.per_minute {
            self.sent.pop_front();
        }
        at.saturating_duration_since(now)
    }

    /// A 429 told us to back off: nothing may go out before `now + wait`.
    pub fn hold(&mut self, now: Instant, wait: Duration) {
        self.sent.push_back(now + wait);
        while self.sent.len() > self.per_minute {
            self.sent.pop_front();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spaces_requests_one_second_apart() {
        let t0 = Instant::now();
        let mut l = Limiter::mal();
        assert_eq!(l.acquire(t0), Duration::ZERO);
        assert_eq!(l.acquire(t0), Duration::from_secs(1));
        assert_eq!(l.acquire(t0), Duration::from_secs(2));
        // Later, with the gap already passed, no wait.
        assert_eq!(l.acquire(t0 + Duration::from_secs(10)), Duration::ZERO);
    }

    #[test]
    fn never_more_than_twenty_a_minute() {
        let t0 = Instant::now();
        let mut l = Limiter::mal();
        let mut now = t0;
        let mut times = Vec::new();
        for _ in 0..45 {
            now += l.acquire(now);
            times.push(now);
        }
        for w in times.windows(21) {
            assert!(w[20] - w[0] >= Duration::from_secs(60), "21 requests inside a minute");
        }
        for w in times.windows(2) {
            assert!(w[1] - w[0] >= Duration::from_secs(1));
        }
    }

    #[test]
    fn hold_delays_the_next_request() {
        let t0 = Instant::now();
        let mut l = Limiter::mal();
        l.hold(t0, Duration::from_secs(30));
        assert!(l.acquire(t0) >= Duration::from_secs(31));
    }
}
