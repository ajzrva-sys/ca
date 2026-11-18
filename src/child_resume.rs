//! Opt-in continuation of a rate-limited child. Default off. At most two
//! automatic attempts, counted before the attempt runs. A user stop stays stopped.

use std::time::Duration;

pub const MAX_AUTO_ATTEMPTS: u32 = 2;
const MAX_WAIT: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resume {
    Wait(Duration),
    Stay,
}

#[derive(Clone, Debug)]
pub struct Gate {
    enabled: bool,
    nested: bool,
    attempts: u32,
    stopped_by_user: bool,
}

impl Gate {
    pub fn new(enabled: bool, depth: u8) -> Self {
        Self {
            enabled,
            nested: depth >= 2,
            attempts: 0,
            stopped_by_user: false,
        }
    }

    pub fn user_stop(&mut self) {
        self.stopped_by_user = true;
    }

    /// Decide before the next attempt. A rate-limited child that is allowed to
    /// continue consumes one of its two attempts here.
    pub fn admit(&mut self, message: &str) -> Resume {
        if !self.enabled || self.nested || self.stopped_by_user {
            return Resume::Stay;
        }
        let Some(wait) = rate_limit_wait(message) else {
            return Resume::Stay;
        };
        if self.attempts >= MAX_AUTO_ATTEMPTS {
            return Resume::Stay;
        }
        self.attempts = self.attempts.saturating_add(1);
        Resume::Wait(wait)
    }

    #[cfg(test)]
    pub fn attempts(&self) -> u32 {
        self.attempts
    }
}

pub fn rate_limit_wait(message: &str) -> Option<Duration> {
    let lower = message.to_ascii_lowercase();
    let limited = lower.contains("429")
        || lower.contains("rate limit")
        || lower.contains("rate_limit")
        || lower.contains("too many requests");
    if !limited {
        return None;
    }
    Some(parse_wait(&lower).min(MAX_WAIT))
}

fn parse_wait(message: &str) -> Duration {
    for marker in ["retry-after", "retry after", "retry in"] {
        if let Some(rest) = message.split(marker).nth(1) {
            let digits: String = rest
                .chars()
                .skip_while(|ch| !ch.is_ascii_digit())
                .take_while(|ch| ch.is_ascii_digit())
                .collect();
            if let Ok(seconds) = digits.parse::<u64>() {
                return Duration::from_secs(seconds.max(1));
            }
        }
    }
    Duration::from_secs(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limited_child_resumes_twice_when_opted_in() {
        let mut gate = Gate::new(false, 1);
        assert_eq!(
            gate.admit("provider HTTP 429: rate limit, retry after 5"),
            Resume::Stay
        );
        assert_eq!(gate.attempts(), 0);

        let mut gate = Gate::new(true, 1);
        assert_eq!(
            gate.admit("provider HTTP 429: too many requests, retry-after 5"),
            Resume::Wait(Duration::from_secs(5))
        );
        assert_eq!(gate.attempts(), 1);
        assert_eq!(
            gate.admit("rate_limit"),
            Resume::Wait(Duration::from_secs(1))
        );
        assert_eq!(gate.attempts(), 2);
        assert_eq!(gate.admit("429"), Resume::Stay);
        assert_eq!(gate.attempts(), 2);

        let mut nested = Gate::new(true, 2);
        assert_eq!(nested.admit("429"), Resume::Stay);

        let mut stopped = Gate::new(true, 1);
        stopped.user_stop();
        assert_eq!(stopped.admit("429"), Resume::Stay);
        assert_eq!(stopped.attempts(), 0);

        assert_eq!(rate_limit_wait("connection reset"), None);
    }
}
