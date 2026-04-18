use anyhow::Result;
use rusqlite::Connection;

pub struct Analytics {
    conn: Connection,
}

impl Analytics {
    pub fn new(db_path: &str) -> Result<Self> {
        let conn = Connection::open(db_path)?;

        // Create analytics table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS query_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                query TEXT NOT NULL,
                timestamp INTEGER NOT NULL,
                results_count INTEGER NOT NULL,
                search_time_ms REAL NOT NULL,
                mode TEXT NOT NULL
            )",
            [],
        )?;

        // Create index for faster queries
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_query_logs_timestamp ON query_logs(timestamp)",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_query_logs_query ON query_logs(query)",
            [],
        )?;

        Ok(Self { conn })
    }

    pub fn log_query(
        &self,
        query: &str,
        results_count: usize,
        search_time_ms: f64,
        mode: &str,
    ) -> Result<()> {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;

        self.conn.execute(
            "INSERT INTO query_logs (query, timestamp, results_count, search_time_ms, mode) VALUES (?, ?, ?, ?, ?)",
            (query, timestamp, results_count as i64, search_time_ms, mode),
        )?;

        Ok(())
    }

    pub fn get_top_queries(&self, limit: usize) -> Result<Vec<(String, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT query, COUNT(*) as count FROM query_logs GROUP BY query ORDER BY count DESC LIMIT ?"
        )?;

        let results = stmt.query_map([limit], |row| Ok((row.get(0)?, row.get(1)?)))?;

        Ok(results.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn get_failed_searches(&self, limit: usize) -> Result<Vec<(String, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT query, COUNT(*) as count FROM query_logs WHERE results_count = 0 GROUP BY query ORDER BY count DESC LIMIT ?"
        )?;

        let results = stmt.query_map([limit], |row| Ok((row.get(0)?, row.get(1)?)))?;

        Ok(results.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn get_avg_search_time(&self) -> Result<f64> {
        let avg: f64 =
            self.conn
                .query_row("SELECT AVG(search_time_ms) FROM query_logs", [], |row| {
                    row.get(0)
                })?;

        Ok(avg)
    }

    pub fn get_queries_by_mode(&self) -> Result<Vec<(String, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT mode, COUNT(*) as count FROM query_logs GROUP BY mode ORDER BY count DESC",
        )?;

        let results = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;

        Ok(results.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn get_total_queries(&self) -> Result<i64> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM query_logs", [], |row| row.get(0))?;

        Ok(count)
    }
}
