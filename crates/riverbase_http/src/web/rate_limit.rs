//! Simple in-process rate limit for `:hook` and auth-adjacent routes ([SUR-09]).

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::base::RiverbaseResult;

const DEFAULT_WINDOW: Duration = Duration::from_secs(60);
const DEFAULT_MAX: u32 = 30;

struct Window {
    started: Instant,
    count: u32,
}

struct RateLimiter {
    windows: Mutex<HashMap<String, Window>>,
    window: Duration,
    max: u32,
}

impl RateLimiter {
    fn check(&self, key: &str) -> RiverbaseResult<()> {
        let now = Instant::now();
        let mut windows = self.windows.lock().expect("rate limit lock");
        windows.retain(|_, slot| now.duration_since(slot.started) < self.window);
        let slot = windows.entry(key.to_string()).or_insert(Window {
            started: now,
            count: 0,
        });
        if now.duration_since(slot.started) >= self.window {
            slot.started = now;
            slot.count = 0;
        }
        if slot.count >= self.max {
            return Err(crate::errors::WEB_104.with_data(
                serde_json::json!({ "limit": self.max, "window_secs": self.window.as_secs() }),
            ));
        }
        slot.count += 1;
        Ok(())
    }
}

fn hook_limiter() -> &'static RateLimiter {
    static LIMITER: OnceLock<RateLimiter> = OnceLock::new();
    LIMITER.get_or_init(|| RateLimiter {
        windows: Mutex::new(HashMap::new()),
        window: DEFAULT_WINDOW,
        max: DEFAULT_MAX,
    })
}

/// Rate-limit a `:hook` invocation. `key` is typically `cmdkey` plus a coarse client identity.
pub fn check_hook_rate(key: &str) -> RiverbaseResult<()> {
    hook_limiter().check(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn burst_is_throttled() {
        let limiter = RateLimiter {
            windows: Mutex::new(HashMap::new()),
            window: Duration::from_secs(60),
            max: 2,
        };
        limiter.check("hook:pay").unwrap();
        limiter.check("hook:pay").unwrap();
        let err = limiter.check("hook:pay").expect_err("throttled");
        assert_eq!(err.errcode.as_str(), "WEB-104");
        assert_eq!(err.http_status, 429);
    }
}
