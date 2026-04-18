use anyhow::Result;
use rusqlite::{Connection, OptionalExtension};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use url::Url;

pub struct RateLimiter<'a> {
    conn: &'a Connection,
}

impl<'a> RateLimiter<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Extract domain from URL
    pub fn extract_domain(url: &str) -> Result<String> {
        let parsed = Url::parse(url)?;
        let domain = parsed
            .host_str()
            .ok_or_else(|| anyhow::anyhow!("No host in URL"))?
            .to_lowercase();
        Ok(domain)
    }

    /// Get current delay for a domain (in milliseconds)
    pub fn get_delay(&self, domain: &str) -> Result<u64> {
        let delay: Option<i64> = self
            .conn
            .query_row(
                "SELECT rate_limit_delay FROM crawl_state WHERE domain = ?1",
                [domain],
                |row| row.get(0),
            )
            .optional()?;

        Ok(delay.unwrap_or(100) as u64)
    }

    /// Check if we can crawl this domain now, if not return how long to wait
    pub fn can_crawl(&self, domain: &str) -> Result<Option<Duration>> {
        let last_request: Option<i64> = self
            .conn
            .query_row(
                "SELECT last_request_time FROM crawl_state WHERE domain = ?1",
                [domain],
                |row| row.get(0),
            )
            .optional()?;

        let delay_ms = self.get_delay(domain)?;

        if let Some(last) = last_request {
            let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as i64;
            let elapsed = now - last;
            let required_delay = delay_ms as i64;

            if elapsed < required_delay {
                let wait_time = (required_delay - elapsed) as u64;
                return Ok(Some(Duration::from_millis(wait_time)));
            }
        }

        Ok(None)
    }

    /// Record that we made a request to this domain
    pub fn record_request(&self, domain: &str) -> Result<()> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis() as i64;

        self.conn.execute(
            "INSERT INTO crawl_state (domain, last_request_time, error_count, rate_limit_delay)
             VALUES (?1, ?2, 0, 100)
             ON CONFLICT(domain) DO UPDATE SET last_request_time = ?2",
            (domain, now),
        )?;

        Ok(())
    }

    /// Wait until we can crawl this domain
    pub async fn wait_if_needed(&self, url: &str) -> Result<()> {
        let domain = Self::extract_domain(url)?;

        if let Some(wait_time) = self.can_crawl(&domain)? {
            tokio::time::sleep(wait_time).await;
        }

        self.record_request(&domain)?;
        Ok(())
    }

    /// Record an error for this domain and increase rate limit delay
    pub fn record_error(&self, domain: &str, status_code: u16) -> Result<()> {
        // Only increase delay for rate limit and server errors
        if !matches!(status_code, 403 | 429 | 500..=599) {
            return Ok(());
        }

        let current_delay = self.get_delay(domain)?;
        let current_errors: i64 = self
            .conn
            .query_row(
                "SELECT error_count FROM crawl_state WHERE domain = ?1",
                [domain],
                |row| row.get(0),
            )
            .unwrap_or(0);

        // Exponential backoff: double the delay, max 10 seconds
        let new_delay = (current_delay * 2).min(10_000);
        let new_error_count = current_errors + 1;

        self.conn.execute(
            "INSERT INTO crawl_state (domain, error_count, rate_limit_delay, last_request_time)
             VALUES (?1, ?2, ?3, NULL)
             ON CONFLICT(domain) DO UPDATE SET 
                error_count = ?2,
                rate_limit_delay = ?3",
            (domain, new_error_count, new_delay as i64),
        )?;

        Ok(())
    }

    /// Record a successful request and potentially reduce rate limit delay
    pub fn record_success(&self, domain: &str) -> Result<()> {
        let current_delay = self.get_delay(domain)?;
        let current_errors: i64 = self
            .conn
            .query_row(
                "SELECT error_count FROM crawl_state WHERE domain = ?1",
                [domain],
                |row| row.get(0),
            )
            .unwrap_or(0);

        // Reset error count on success
        let new_error_count = 0;

        // Gradually reduce delay back to minimum (100ms) after successful requests
        let new_delay = if current_errors > 0 {
            // Had errors, keep current delay but reset error count
            current_delay
        } else if current_delay > 100 {
            // No recent errors, slowly reduce delay
            (current_delay / 2).max(100)
        } else {
            100
        };

        self.conn.execute(
            "INSERT INTO crawl_state (domain, error_count, rate_limit_delay, last_request_time)
             VALUES (?1, ?2, ?3, NULL)
             ON CONFLICT(domain) DO UPDATE SET 
                error_count = ?2,
                rate_limit_delay = ?3",
            (domain, new_error_count, new_delay as i64),
        )?;

        Ok(())
    }

    /// Get error count for a domain
    pub fn get_error_count(&self, domain: &str) -> Result<i64> {
        let count: i64 = self
            .conn
            .query_row(
                "SELECT error_count FROM crawl_state WHERE domain = ?1",
                [domain],
                |row| row.get(0),
            )
            .unwrap_or(0);
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    #[test]
    fn test_extract_domain() {
        assert_eq!(
            RateLimiter::extract_domain("https://example.com/path").unwrap(),
            "example.com"
        );

        assert_eq!(
            RateLimiter::extract_domain("https://Example.COM/path").unwrap(),
            "example.com"
        );

        assert_eq!(
            RateLimiter::extract_domain("https://sub.example.com/path").unwrap(),
            "sub.example.com"
        );
    }

    #[test]
    fn test_rate_limiting() {
        let conn = db::init_db(":memory:").unwrap();
        let limiter = RateLimiter::new(&conn);

        let domain = "example.com";

        // First request should be allowed immediately
        assert!(limiter.can_crawl(domain).unwrap().is_none());

        // Record the request
        limiter.record_request(domain).unwrap();

        // Immediate second request should require waiting
        let wait = limiter.can_crawl(domain).unwrap();
        assert!(wait.is_some());
        assert!(wait.unwrap().as_millis() > 0);

        // Check delay is default 100ms
        assert_eq!(limiter.get_delay(domain).unwrap(), 100);
    }

    #[test]
    fn test_multiple_domains() {
        let conn = db::init_db(":memory:").unwrap();
        let limiter = RateLimiter::new(&conn);

        // Record requests to different domains
        limiter.record_request("example.com").unwrap();
        limiter.record_request("test.com").unwrap();

        // Both should have rate limits
        assert!(limiter.can_crawl("example.com").unwrap().is_some());
        assert!(limiter.can_crawl("test.com").unwrap().is_some());

        // Different domain should be independent
        assert!(limiter.can_crawl("other.com").unwrap().is_none());
    }

    #[test]
    fn test_adaptive_rate_limiting() {
        let conn = db::init_db(":memory:").unwrap();
        let limiter = RateLimiter::new(&conn);

        let domain = "example.com";

        // Initial delay should be 100ms
        assert_eq!(limiter.get_delay(domain).unwrap(), 100);

        // Record 403 error - should double delay
        limiter.record_error(domain, 403).unwrap();
        assert_eq!(limiter.get_delay(domain).unwrap(), 200);
        assert_eq!(limiter.get_error_count(domain).unwrap(), 1);

        // Another error - should double again
        limiter.record_error(domain, 429).unwrap();
        assert_eq!(limiter.get_delay(domain).unwrap(), 400);
        assert_eq!(limiter.get_error_count(domain).unwrap(), 2);

        // Record success - should reset error count but keep delay
        limiter.record_success(domain).unwrap();
        assert_eq!(limiter.get_delay(domain).unwrap(), 400);
        assert_eq!(limiter.get_error_count(domain).unwrap(), 0);

        // Another success - should reduce delay
        limiter.record_success(domain).unwrap();
        assert_eq!(limiter.get_delay(domain).unwrap(), 200);

        // Keep succeeding - should eventually reach minimum
        limiter.record_success(domain).unwrap();
        assert_eq!(limiter.get_delay(domain).unwrap(), 100);

        limiter.record_success(domain).unwrap();
        assert_eq!(limiter.get_delay(domain).unwrap(), 100);
    }

    #[test]
    fn test_max_delay_cap() {
        let conn = db::init_db(":memory:").unwrap();
        let limiter = RateLimiter::new(&conn);

        let domain = "example.com";

        // Keep recording errors to test max cap
        for _ in 0..20 {
            limiter.record_error(domain, 503).unwrap();
        }

        // Should cap at 10 seconds (10000ms)
        let delay = limiter.get_delay(domain).unwrap();
        assert_eq!(delay, 10_000);
    }

    #[test]
    fn test_non_error_status_codes() {
        let conn = db::init_db(":memory:").unwrap();
        let limiter = RateLimiter::new(&conn);

        let domain = "example.com";

        // 404 should not increase delay
        limiter.record_error(domain, 404).unwrap();
        assert_eq!(limiter.get_delay(domain).unwrap(), 100);
        assert_eq!(limiter.get_error_count(domain).unwrap(), 0);

        // 200 is handled by record_success, not record_error
        // But if we call record_error with 200, it should not increase delay
        limiter.record_error(domain, 200).unwrap();
        assert_eq!(limiter.get_delay(domain).unwrap(), 100);
    }
}
