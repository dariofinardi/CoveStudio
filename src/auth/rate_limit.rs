//! In-memory rate-limit / lockout for PIN unlock.
//!
//! The single-user threat model: someone with local network access to
//! port 3001 tries to brute-force the PIN. Argon2id makes each attempt
//! cost ~100 ms of CPU, but at 10 attempts/s/core a 4-digit space
//! (10 000) falls in ~17 minutes. This module enforces a hard cap.
//!
//! ## Policy
//!
//! Keyed by client IP (the only stable identifier we have without
//! adding cookies). Each key keeps a small counter:
//!
//!   - On every **failed** unlock: counter++ and timestamp updated.
//!   - After **5** consecutive failures: subsequent attempts are
//!     refused with `429 Too Many Requests` + `Retry-After: 60`
//!     until 60 s of inactivity have passed.
//!   - A **successful** unlock resets the counter for that key.
//!
//! 60 s lockout is a deliberate compromise: long enough to make
//! brute-force impractical (10 000 attempts × 60 s ÷ 5 = 33 hours
//! per attempt-batch) but short enough that a user who fat-fingered
//! their PIN can retry within a minute without panic.
//!
//! ## What this is NOT
//!
//! - Not a distributed rate limit. Counter lives in-process; restart
//!   resets it. Acceptable for a single-instance local-only app.
//! - Not a DoS shield. A flooder can spawn many IPs (NAT'd or otherwise);
//!   that's an external problem (firewall, ufw). The goal here is
//!   *credential* protection, not connection-level fairness.
//! - Not a replacement for stronger PINs. 6+ digits + this lockout
//!   beats 4-digit + lockout by orders of magnitude.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Maximum consecutive failed unlock attempts before lockout kicks in.
pub const MAX_ATTEMPTS: u32 = 5;

/// How long a key remains locked after `MAX_ATTEMPTS` failures (s).
pub const LOCKOUT_SECS: u64 = 60;

#[derive(Debug, Clone, Copy)]
struct Counter {
    failures: u32,
    last_failure: Instant,
}

#[derive(Debug, Default)]
pub struct UnlockRateLimiter {
    inner: Mutex<HashMap<IpAddr, Counter>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckOutcome {
    /// The caller may proceed with the unlock attempt.
    Allow,
    /// The caller is locked out; this many seconds remain on the wall.
    Deny { retry_after_secs: u64 },
}

impl UnlockRateLimiter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Check whether `ip` is permitted to attempt an unlock right now.
    /// Returns `Deny` if a previous failure-burst put it in lockout
    /// AND less than `LOCKOUT_SECS` have elapsed since the last failure.
    pub fn check(&self, ip: IpAddr) -> CheckOutcome {
        let mut guard = self.inner.lock().expect("lock poisoned");
        match guard.get(&ip) {
            Some(c) if c.failures >= MAX_ATTEMPTS => {
                let elapsed = c.last_failure.elapsed();
                if elapsed.as_secs() < LOCKOUT_SECS {
                    let remaining = LOCKOUT_SECS - elapsed.as_secs();
                    CheckOutcome::Deny {
                        retry_after_secs: remaining,
                    }
                } else {
                    // Lockout expired — wipe and let through.
                    guard.remove(&ip);
                    CheckOutcome::Allow
                }
            }
            _ => CheckOutcome::Allow,
        }
    }

    /// Record a failed unlock. Increments the counter and pins the
    /// timestamp; the next `check` decides whether to lock out.
    pub fn record_failure(&self, ip: IpAddr) {
        let mut guard = self.inner.lock().expect("lock poisoned");
        let entry = guard.entry(ip).or_insert(Counter {
            failures: 0,
            last_failure: Instant::now(),
        });
        entry.failures = entry.failures.saturating_add(1);
        entry.last_failure = Instant::now();
    }

    /// Record a successful unlock. Wipes the counter for that key so
    /// the user doesn't carry stale failures across a successful login.
    pub fn record_success(&self, ip: IpAddr) {
        let mut guard = self.inner.lock().expect("lock poisoned");
        guard.remove(&ip);
    }

    /// Test-only: introspect the counter so tests can assert state
    /// without forcing the public API to expose internals.
    #[cfg(test)]
    fn snapshot(&self, ip: IpAddr) -> Option<(u32, Duration)> {
        let guard = self.inner.lock().expect("lock poisoned");
        guard.get(&ip).map(|c| (c.failures, c.last_failure.elapsed()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn ip(b: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(192, 168, 0, b))
    }

    #[test]
    fn empty_limiter_allows() {
        let l = UnlockRateLimiter::new();
        assert_eq!(l.check(ip(1)), CheckOutcome::Allow);
    }

    #[test]
    fn record_success_clears_counter() {
        let l = UnlockRateLimiter::new();
        l.record_failure(ip(1));
        l.record_failure(ip(1));
        assert_eq!(l.snapshot(ip(1)).unwrap().0, 2);
        l.record_success(ip(1));
        assert!(l.snapshot(ip(1)).is_none());
        assert_eq!(l.check(ip(1)), CheckOutcome::Allow);
    }

    #[test]
    fn five_failures_lock_out() {
        let l = UnlockRateLimiter::new();
        for _ in 0..MAX_ATTEMPTS {
            l.record_failure(ip(1));
        }
        match l.check(ip(1)) {
            CheckOutcome::Deny { retry_after_secs } => {
                assert!(retry_after_secs > 0);
                assert!(retry_after_secs <= LOCKOUT_SECS);
            }
            CheckOutcome::Allow => panic!("expected Deny after MAX_ATTEMPTS failures"),
        }
    }

    #[test]
    fn keys_are_isolated_per_ip() {
        let l = UnlockRateLimiter::new();
        for _ in 0..MAX_ATTEMPTS {
            l.record_failure(ip(1));
        }
        assert!(matches!(l.check(ip(1)), CheckOutcome::Deny { .. }));
        // Different IP is unaffected.
        assert_eq!(l.check(ip(2)), CheckOutcome::Allow);
    }

    #[test]
    fn four_failures_still_allow() {
        let l = UnlockRateLimiter::new();
        for _ in 0..(MAX_ATTEMPTS - 1) {
            l.record_failure(ip(1));
        }
        assert_eq!(l.check(ip(1)), CheckOutcome::Allow);
    }
}
