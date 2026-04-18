use anyhow::Result;
use rusqlite::{Connection, OptionalExtension};

pub fn init_db(path: &str) -> Result<Connection> {
    let conn = Connection::open(path)?;

    // Set busy timeout to 30 seconds
    conn.busy_timeout(std::time::Duration::from_secs(30))?;

    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS pages (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            url TEXT UNIQUE NOT NULL,
            html_content TEXT,
            status_code INTEGER,
            content_hash TEXT,
            crawled_at INTEGER NOT NULL
        );
        
        CREATE TABLE IF NOT EXISTS links (
            from_url TEXT NOT NULL,
            to_url TEXT NOT NULL,
            PRIMARY KEY (from_url, to_url)
        );
        
        CREATE TABLE IF NOT EXISTS crawl_queue (
            url TEXT PRIMARY KEY,
            discovered_at INTEGER NOT NULL,
            priority INTEGER DEFAULT 0
        );
        
        CREATE TABLE IF NOT EXISTS crawl_state (
            domain TEXT PRIMARY KEY,
            last_request_time INTEGER,
            error_count INTEGER DEFAULT 0,
            rate_limit_delay INTEGER DEFAULT 20
        );
        
        CREATE INDEX IF NOT EXISTS idx_pages_url ON pages(url);
        CREATE INDEX IF NOT EXISTS idx_pages_hash ON pages(content_hash);
        CREATE INDEX IF NOT EXISTS idx_queue_priority ON crawl_queue(priority DESC);
        ",
    )?;

    // Enable WAL mode and performance optimizations
    conn.execute_batch(
        "
        PRAGMA journal_mode=WAL;
        PRAGMA synchronous=NORMAL;
        PRAGMA cache_size=-64000;
        PRAGMA temp_store=MEMORY;
        PRAGMA mmap_size=30000000000;
        PRAGMA wal_autocheckpoint=1000;
        ",
    )?;

    Ok(conn)
}

/// Check if content hash already exists (duplicate content)
pub fn is_duplicate_content(conn: &Connection, content_hash: &str) -> Result<bool> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM pages WHERE content_hash = ?1)",
        [content_hash],
        |row| row.get(0),
    )?;
    Ok(exists)
}

/// Get URL that has this content hash (for logging)
pub fn get_url_by_hash(conn: &Connection, content_hash: &str) -> Result<Option<String>> {
    let url: Option<String> = conn
        .query_row(
            "SELECT url FROM pages WHERE content_hash = ?1 LIMIT 1",
            [content_hash],
            |row| row.get(0),
        )
        .optional()?;
    Ok(url)
}

/// Execute with retry on database locked errors
pub fn execute_with_retry<F, T>(mut f: F, max_retries: u32) -> Result<T>
where
    F: FnMut() -> Result<T>,
{
    let mut retries = 0;
    loop {
        match f() {
            Ok(result) => return Ok(result),
            Err(e) => {
                let err_str = e.to_string();
                if err_str.contains("database is locked") && retries < max_retries {
                    retries += 1;
                    std::thread::sleep(std::time::Duration::from_millis(50 * retries as u64));
                } else {
                    return Err(e);
                }
            }
        }
    }
}
