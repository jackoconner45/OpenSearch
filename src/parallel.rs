use anyhow::Result;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Semaphore;
use tracing::{error, info, warn};

use crate::{
    checkpoint::CheckpointManager,
    crawler::{CrawlResult, Crawler},
    db,
    queue::CrawlQueue,
    rate_limiter::RateLimiter,
};

pub struct ParallelCrawler {
    max_concurrent: usize,
    max_pages: usize,
}

impl ParallelCrawler {
    pub fn new(max_concurrent: usize, max_pages: usize) -> Self {
        Self {
            max_concurrent,
            max_pages,
        }
    }

    pub async fn run(&self, db_path: &str, seed_urls: Vec<String>) -> Result<()> {
        info!(
            "Starting parallel crawler: {} workers, {} max pages",
            self.max_concurrent, self.max_pages
        );

        let conn = db::init_db(db_path)?;

        // Add seed URLs to queue
        {
            let queue = CrawlQueue::new(&conn);
            for url in seed_urls {
                queue.add_url(&url, 10)?;
            }
            info!("Added {} seed URLs to queue", queue.size()?);
        }

        let crawler = Arc::new(Crawler::new()?);
        let semaphore = Arc::new(Semaphore::new(self.max_concurrent));
        let mut checkpoint_mgr = CheckpointManager::new(200); // Checkpoint every 200 pages
        let mut tasks = Vec::new();

        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_clone = Arc::clone(&shutdown);

        // Setup Ctrl+C handler
        tokio::spawn(async move {
            tokio::signal::ctrl_c().await.ok();
            info!("Shutdown signal received, finishing current tasks...");
            shutdown_clone.store(true, Ordering::Relaxed);
        });

        let start_time = std::time::Instant::now();
        let mut last_progress = std::time::Instant::now();

        loop {
            // Check shutdown
            if shutdown.load(Ordering::Relaxed) {
                info!("Shutting down gracefully...");
                break;
            }

            // Check if done
            let crawled = conn.query_row("SELECT COUNT(*) FROM pages", [], |row| {
                row.get::<_, usize>(0)
            })?;
            if crawled >= self.max_pages {
                info!("Reached max pages limit");
                break;
            }

            // Progress logging every 30 seconds
            if last_progress.elapsed().as_secs() >= 30 {
                let queue = CrawlQueue::new(&conn);
                let queue_size = queue.size()?;
                let stats = checkpoint_mgr.stats();
                let elapsed = start_time.elapsed();
                let rate = if elapsed.as_secs() > 0 {
                    stats.total_pages as f64 / elapsed.as_secs() as f64
                } else {
                    0.0
                };

                info!(
                    "Progress: {} pages, {} skipped, {} errors, {} queued, {:.1} pages/sec",
                    stats.total_pages, stats.total_skipped, stats.total_errors, queue_size, rate
                );
                last_progress = std::time::Instant::now();
            }

            // Get next URL
            let url = {
                let queue = CrawlQueue::new(&conn);
                match queue.pop_next()? {
                    Some(u) => u,
                    None => {
                        if tasks.is_empty() {
                            info!("Queue empty and no pending tasks");
                            break;
                        }
                        // Wait for tasks to complete and potentially add more URLs
                        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                        continue;
                    }
                }
            };

            let crawler = Arc::clone(&crawler);
            let semaphore = Arc::clone(&semaphore);
            let db_path = db_path.to_string();

            let task = tokio::spawn(async move {
                let _permit = semaphore.acquire().await.unwrap();

                let conn = match db::init_db(&db_path) {
                    Ok(c) => c,
                    Err(e) => {
                        error!("Failed to open DB: {}", e);
                        return false;
                    }
                };

                let domain = match RateLimiter::extract_domain(&url) {
                    Ok(d) => d,
                    Err(_) => return false,
                };

                let wait_time = {
                    let limiter = RateLimiter::new(&conn);
                    limiter.can_crawl(&domain).unwrap_or(None)
                };

                if let Some(delay) = wait_time {
                    tokio::time::sleep(delay).await;
                }

                {
                    let limiter = RateLimiter::new(&conn);
                    let _ = limiter.record_request(&domain);
                }

                let mut retries = 0;
                let max_retries = 1;
                let mut success = false;

                while retries <= max_retries && !success {
                    match crawler.fetch(&url).await {
                        Ok(result) => {
                            if db::is_duplicate_content(&conn, &result.content_hash)
                                .unwrap_or(false)
                            {
                                return false;
                            }

                            if let Err(e) = save_crawl_result(&conn, &result) {
                                error!("Save error: {}", e);
                                return false;
                            }

                            let limiter = RateLimiter::new(&conn);
                            if result.status_code == 200 {
                                let _ = limiter.record_success(&domain);
                            } else if matches!(result.status_code, 403 | 429 | 500..=599) {
                                let _ = limiter.record_error(&domain, result.status_code);
                            }

                            success = true;
                        }
                        Err(e) => {
                            if retries < max_retries {
                                warn!(
                                    "Fetch error for {} (retry {}/{}): {}",
                                    url,
                                    retries + 1,
                                    max_retries,
                                    e
                                );
                                tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
                                retries += 1;
                            } else {
                                warn!(
                                    "Fetch failed for {} after {} retries: {}",
                                    url, max_retries, e
                                );
                                let limiter = RateLimiter::new(&conn);
                                let _ = limiter.record_error(&domain, 500);
                                return false;
                            }
                        }
                    }
                }

                success
            });

            tasks.push(task);

            if tasks.len() >= self.max_concurrent * 2 {
                let mut i = 0;
                while i < tasks.len() {
                    if tasks[i].is_finished() {
                        let task = tasks.remove(i);
                        if let Ok(success) = task.await {
                            if success {
                                checkpoint_mgr.record_success();
                            } else {
                                checkpoint_mgr.record_skip();
                            }
                        }
                    } else {
                        i += 1;
                    }
                }

                if checkpoint_mgr.should_checkpoint() {
                    let queue = CrawlQueue::new(&conn);
                    let queue_size = queue.size()?;
                    checkpoint_mgr.checkpoint(&conn, queue_size)?;
                }
            }
        }

        info!("Waiting for {} pending tasks to complete...", tasks.len());
        for task in tasks {
            if let Ok(success) = task.await {
                if success {
                    checkpoint_mgr.record_success();
                } else {
                    checkpoint_mgr.record_skip();
                }
            }
        }

        let stats = checkpoint_mgr.stats();
        let elapsed = start_time.elapsed();

        let crawl_stats = crate::stats::get_crawl_stats(
            &conn,
            elapsed.as_secs_f64(),
            stats.total_pages,
            stats.total_skipped,
            stats.total_errors,
        )?;

        crate::stats::print_final_stats(&crawl_stats);

        Ok(())
    }
}

fn save_crawl_result(conn: &rusqlite::Connection, result: &CrawlResult) -> Result<()> {
    // Use retry logic for database operations
    crate::db::execute_with_retry(
        || {
            conn.execute(
            "INSERT OR REPLACE INTO pages (url, html_content, status_code, content_hash, crawled_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            (
                &result.url,
                &result.html,
                result.status_code,
                &result.content_hash,
                result.crawled_at,
            ),
        )?;

            for link in &result.links {
                conn.execute(
                    "INSERT OR IGNORE INTO links (from_url, to_url) VALUES (?1, ?2)",
                    (&result.url, link),
                )?;

                conn.execute(
                "INSERT OR IGNORE INTO crawl_queue (url, discovered_at, priority) VALUES (?1, ?2, 0)",
                (link, result.crawled_at),
            )?;
            }

            Ok(())
        },
        5,
    )
}
