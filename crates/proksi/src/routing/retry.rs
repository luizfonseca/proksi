//! Retries: a request sent to another upstream when one can't be reached (`connect`), or
//! answers with a status of `on`, `attempts` times at most. Statuses are retried for idempotent
//! requests only, whose body was kept to send again, and before anything was sent to the client.

use crate::config::host::Retry;

/// A location's `retry { ... }`, ready to decide
#[derive(Debug, Clone, PartialEq)]
pub struct Policy {
    /// Tries after the first
    attempts: u8,
    connect: bool,
    statuses: Vec<u16>,
}

/// Retries after the first try, at most
pub const MAX_ATTEMPTS: u8 = 10;

impl Policy {
    pub fn new(retry: &Retry) -> Result<Self, String> {
        if retry.attempts == 0 || retry.attempts > MAX_ATTEMPTS {
            return Err(format!("retry: attempts is 1 to {MAX_ATTEMPTS}"));
        }
        let mut connect = false;
        let mut statuses = vec![];
        for on in &retry.on {
            if on == "connect" {
                connect = true;
                continue;
            }
            match on.parse::<u16>() {
                Ok(status) if (400..=599).contains(&status) => statuses.push(status),
                _ => {
                    return Err(format!(
                        "retry: on \"{on}\" is not \"connect\" or a status from 400 to 599"
                    ))
                }
            }
        }
        Ok(Self {
            attempts: retry.attempts,
            connect,
            statuses,
        })
    }

    /// Whether a request tried `tries` times, a rule allowing (`ruled`), can be tried again
    fn left(&self, tries: u8, ruled: Option<bool>) -> bool {
        ruled != Some(false) && tries <= self.attempts
    }

    /// Whether to try again after the upstream couldn't be reached
    pub fn on_connect(&self, tries: u8, ruled: Option<bool>) -> bool {
        self.connect && self.left(tries, ruled)
    }

    /// Whether to try again after the upstream answered `status`
    pub fn on_status(&self, status: u16, tries: u8, ruled: Option<bool>) -> bool {
        self.statuses.contains(&status) && self.left(tries, ruled)
    }
}

#[cfg(test)]
mod tests {
    use crate::config::host::Retry;

    use super::Policy;

    fn policy(attempts: u8, on: &[&str]) -> Result<Policy, String> {
        Policy::new(&Retry {
            attempts,
            on: on.iter().map(ToString::to_string).collect(),
        })
    }

    #[test]
    fn test_policy() {
        let retry = policy(2, &["connect", "502"]).unwrap();
        // First try, then 2 more
        assert!(retry.on_connect(1, None) && retry.on_connect(2, None));
        assert!(!retry.on_connect(3, None));
        assert!(retry.on_status(502, 1, None));
        assert!(!retry.on_status(503, 1, None));
        // Rules turn retries off
        assert!(!retry.on_connect(1, Some(false)));
        assert!(retry.on_connect(1, Some(true)));

        let statuses = policy(1, &["503"]).unwrap();
        assert!(!statuses.on_connect(1, None));

        assert_eq!(
            policy(0, &[]).err().as_deref(),
            Some("retry: attempts is 1 to 10")
        );
        assert_eq!(
            policy(1, &["200"]).err().as_deref(),
            Some("retry: on \"200\" is not \"connect\" or a status from 400 to 599")
        );
        assert_eq!(
            Policy::new(&Retry {
                attempts: Retry::default_attempts(),
                on: Retry::default_on()
            }),
            policy(1, &["connect", "502", "503", "504"])
        );
    }
}
