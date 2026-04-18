use anyhow::Result;
use rusqlite::{Connection, OptionalExtension};
use url::Url;

pub struct CrawlQueue<'a> {
    conn: &'a Connection,
}

impl<'a> CrawlQueue<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Normalize URL: lowercase scheme/domain, remove fragment, sort query params
    pub fn normalize_url(url: &str) -> Result<String> {
        let mut parsed = Url::parse(url)?;

        // Remove fragment
        parsed.set_fragment(None);

        // Lowercase scheme and host
        if let Some(host) = parsed.host_str() {
            parsed.set_host(Some(&host.to_lowercase()))?;
        }
        parsed
            .set_scheme(&parsed.scheme().to_lowercase())
            .map_err(|_| anyhow::anyhow!("Invalid scheme"))?;

        // Lowercase and clean path
        let path = parsed.path().to_lowercase();
        let cleaned_path = if path.ends_with('/') && path.len() > 1 {
            path.trim_end_matches('/').to_string()
        } else {
            path
        };
        parsed.set_path(&cleaned_path);

        Ok(parsed.to_string())
    }

    /// Add URL to queue if not already crawled or queued
    pub fn add_url(&self, url: &str, priority: i32) -> Result<bool> {
        let normalized = Self::normalize_url(url)?;

        // Check if already crawled
        let already_crawled: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM pages WHERE url = ?1)",
            [&normalized],
            |row| row.get(0),
        )?;

        if already_crawled {
            return Ok(false);
        }

        // Try to insert into queue
        let inserted = self.conn.execute(
            "INSERT OR IGNORE INTO crawl_queue (url, discovered_at, priority) 
             VALUES (?1, ?2, ?3)",
            (
                &normalized,
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_secs(),
                priority,
            ),
        )?;

        Ok(inserted > 0)
    }

    /// Pop next URL from queue (highest priority first)
    pub fn pop_next(&self) -> Result<Option<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT url FROM crawl_queue ORDER BY priority DESC, discovered_at ASC LIMIT 1",
        )?;

        let url: Option<String> = stmt.query_row([], |row| row.get(0)).optional()?;

        if let Some(ref u) = url {
            self.conn
                .execute("DELETE FROM crawl_queue WHERE url = ?1", [u])?;
        }

        Ok(url)
    }

    /// Check if URL has already been crawled
    pub fn is_crawled(&self, url: &str) -> Result<bool> {
        let normalized = Self::normalize_url(url)?;
        let exists: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM pages WHERE url = ?1)",
            [&normalized],
            |row| row.get(0),
        )?;
        Ok(exists)
    }

    /// Get queue size
    pub fn size(&self) -> Result<usize> {
        let count: usize = self
            .conn
            .query_row("SELECT COUNT(*) FROM crawl_queue", [], |row| row.get(0))?;
        Ok(count)
    }

    /// Get total crawled pages count
    pub fn crawled_count(&self) -> Result<usize> {
        let count: usize = self
            .conn
            .query_row("SELECT COUNT(*) FROM pages", [], |row| row.get(0))?;
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    #[test]
    fn test_normalize_url() {
        assert_eq!(
            CrawlQueue::normalize_url("https://Example.COM/path").unwrap(),
            "https://example.com/path"
        );

        assert_eq!(
            CrawlQueue::normalize_url("https://example.com/path#fragment").unwrap(),
            "https://example.com/path"
        );

        assert_eq!(
            CrawlQueue::normalize_url("https://example.com/path/").unwrap(),
            "https://example.com/path"
        );

        assert_eq!(
            CrawlQueue::normalize_url("https://example.com/").unwrap(),
            "https://example.com/"
        );
    }

    #[test]
    fn test_queue_operations() {
        let conn = db::init_db(":memory:").unwrap();
        let queue = CrawlQueue::new(&conn);

        // Add URLs
        assert!(queue.add_url("https://example.com/page1", 0).unwrap());
        assert!(queue.add_url("https://example.com/page2", 10).unwrap());
        assert!(queue.add_url("https://example.com/page3", 5).unwrap());

        // Duplicate should return false
        assert!(!queue.add_url("https://example.com/page1", 0).unwrap());

        // Normalized duplicate should return false
        assert!(!queue
            .add_url("https://Example.COM/page1#fragment", 0)
            .unwrap());

        assert_eq!(queue.size().unwrap(), 3);

        // Pop should return highest priority first
        assert_eq!(
            queue.pop_next().unwrap(),
            Some("https://example.com/page2".to_string())
        );
        assert_eq!(queue.size().unwrap(), 2);

        assert_eq!(
            queue.pop_next().unwrap(),
            Some("https://example.com/page3".to_string())
        );
        assert_eq!(
            queue.pop_next().unwrap(),
            Some("https://example.com/page1".to_string())
        );

        // Queue should be empty
        assert_eq!(queue.pop_next().unwrap(), None);
        assert_eq!(queue.size().unwrap(), 0);
    }

    #[test]
    fn test_crawled_check() {
        let conn = db::init_db(":memory:").unwrap();
        let queue = CrawlQueue::new(&conn);

        // Add to queue
        queue.add_url("https://example.com/page1", 0).unwrap();
        assert_eq!(queue.size().unwrap(), 1);

        // Mark as crawled by inserting into pages
        conn.execute(
            "INSERT INTO pages (url, html_content, status_code, content_hash, crawled_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            ("https://example.com/page1", "", 200, "hash", 0i64),
        )
        .unwrap();

        assert!(queue.is_crawled("https://example.com/page1").unwrap());

        // Try to add again - should fail because it's crawled
        assert!(!queue.add_url("https://example.com/page1", 0).unwrap());

        // Queue size should still be 1 (the original entry)
        assert_eq!(queue.size().unwrap(), 1);
    }
}
