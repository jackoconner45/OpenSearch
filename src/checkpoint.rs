use anyhow::Result;
use rusqlite::Connection;
use std::time::Instant;
use tracing::info;

pub struct CheckpointManager {
    pages_since_checkpoint: usize,
    checkpoint_interval: usize,
    last_checkpoint: Instant,
    total_pages: usize,
    total_skipped: usize,
    total_errors: usize,
}

impl CheckpointManager {
    pub fn new(checkpoint_interval: usize) -> Self {
        Self {
            pages_since_checkpoint: 0,
            checkpoint_interval,
            last_checkpoint: Instant::now(),
            total_pages: 0,
            total_skipped: 0,
            total_errors: 0,
        }
    }

    /// Record a successful crawl
    pub fn record_success(&mut self) {
        self.pages_since_checkpoint += 1;
        self.total_pages += 1;
    }

    /// Record a skipped page (duplicate)
    pub fn record_skip(&mut self) {
        self.total_skipped += 1;
    }

    /// Record an error
    pub fn record_error(&mut self) {
        self.total_errors += 1;
    }

    /// Check if checkpoint is needed
    pub fn should_checkpoint(&self) -> bool {
        self.pages_since_checkpoint >= self.checkpoint_interval
    }

    /// Perform checkpoint - just commit transaction, no heavy operations
    pub fn checkpoint(&mut self, conn: &Connection, queue_size: usize) -> Result<()> {
        let start = Instant::now();

        // WAL checkpoint - incremental, doesn't block
        conn.execute_batch("PRAGMA wal_checkpoint(PASSIVE);")?;

        let elapsed = start.elapsed();

        info!(
            "Checkpoint: {} pages (+{} since last), {} skipped, {} errors, queue: {}, took: {:?}",
            self.total_pages,
            self.pages_since_checkpoint,
            self.total_skipped,
            self.total_errors,
            queue_size,
            elapsed
        );

        self.pages_since_checkpoint = 0;
        self.last_checkpoint = Instant::now();

        Ok(())
    }

    /// Get stats
    pub fn stats(&self) -> CheckpointStats {
        CheckpointStats {
            total_pages: self.total_pages,
            total_skipped: self.total_skipped,
            total_errors: self.total_errors,
            time_since_checkpoint: self.last_checkpoint.elapsed(),
        }
    }
}

pub struct CheckpointStats {
    pub total_pages: usize,
    pub total_skipped: usize,
    pub total_errors: usize,
    pub time_since_checkpoint: std::time::Duration,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    #[test]
    fn test_checkpoint_manager() {
        let mut manager = CheckpointManager::new(100);

        assert!(!manager.should_checkpoint());

        for _ in 0..99 {
            manager.record_success();
        }
        assert!(!manager.should_checkpoint());

        manager.record_success();
        assert!(manager.should_checkpoint());

        let conn = db::init_db(":memory:").unwrap();
        manager.checkpoint(&conn, 500).unwrap();

        assert!(!manager.should_checkpoint());
        assert_eq!(manager.stats().total_pages, 100);
    }

    #[test]
    fn test_stats_tracking() {
        let mut manager = CheckpointManager::new(100);

        manager.record_success();
        manager.record_success();
        manager.record_skip();
        manager.record_error();

        let stats = manager.stats();
        assert_eq!(stats.total_pages, 2);
        assert_eq!(stats.total_skipped, 1);
        assert_eq!(stats.total_errors, 1);
    }
}
