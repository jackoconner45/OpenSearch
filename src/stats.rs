use anyhow::Result;
use rusqlite::Connection;
use std::collections::HashMap;

pub struct CrawlStats {
    pub total_pages: usize,
    pub total_skipped: usize,
    pub total_errors: usize,
    pub queue_size: usize,
    pub elapsed_secs: f64,
    pub pages_per_sec: f64,
    pub domain_stats: Vec<DomainStat>,
}

pub struct DomainStat {
    pub domain: String,
    pub pages: usize,
    pub error_count: i64,
    pub rate_limit_delay: i64,
}

pub fn get_crawl_stats(
    conn: &Connection,
    elapsed_secs: f64,
    checkpoint_pages: usize,
    checkpoint_skipped: usize,
    checkpoint_errors: usize,
) -> Result<CrawlStats> {
    let queue_size: usize =
        conn.query_row("SELECT COUNT(*) FROM crawl_queue", [], |row| row.get(0))?;

    let pages_per_sec = if elapsed_secs > 0.0 {
        checkpoint_pages as f64 / elapsed_secs
    } else {
        0.0
    };

    // Get domain stats
    let mut domain_stats = Vec::new();
    let mut stmt = conn.prepare(
        "SELECT 
            cs.domain,
            COUNT(DISTINCT p.url) as page_count,
            cs.error_count,
            cs.rate_limit_delay
         FROM crawl_state cs
         LEFT JOIN pages p ON LOWER(SUBSTR(p.url, INSTR(p.url, '://') + 3, 
                                     CASE WHEN INSTR(SUBSTR(p.url, INSTR(p.url, '://') + 3), '/') > 0 
                                          THEN INSTR(SUBSTR(p.url, INSTR(p.url, '://') + 3), '/') - 1
                                          ELSE LENGTH(SUBSTR(p.url, INSTR(p.url, '://') + 3))
                                     END)) = cs.domain
         GROUP BY cs.domain
         ORDER BY page_count DESC
         LIMIT 20"
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(DomainStat {
            domain: row.get(0)?,
            pages: row.get(1)?,
            error_count: row.get(2)?,
            rate_limit_delay: row.get(3)?,
        })
    })?;

    for row in rows {
        domain_stats.push(row?);
    }

    Ok(CrawlStats {
        total_pages: checkpoint_pages,
        total_skipped: checkpoint_skipped,
        total_errors: checkpoint_errors,
        queue_size,
        elapsed_secs,
        pages_per_sec,
        domain_stats,
    })
}

pub fn print_final_stats(stats: &CrawlStats) {
    println!("\n╔══════════════════════════════════════════════════════════╗");
    println!("║              CRAWL STATISTICS                            ║");
    println!("╠══════════════════════════════════════════════════════════╣");
    println!(
        "║  Pages crawled:      {:>8}                          ║",
        stats.total_pages
    );
    println!(
        "║  Pages skipped:      {:>8}                          ║",
        stats.total_skipped
    );
    println!(
        "║  Errors:             {:>8}                          ║",
        stats.total_errors
    );
    println!(
        "║  Queue remaining:    {:>8}                          ║",
        stats.queue_size
    );
    println!(
        "║  Time elapsed:       {:>8.1}s                        ║",
        stats.elapsed_secs
    );
    println!(
        "║  Average rate:       {:>8.2} pages/sec               ║",
        stats.pages_per_sec
    );
    println!("╚══════════════════════════════════════════════════════════╝");

    if !stats.domain_stats.is_empty() {
        println!("\n╔══════════════════════════════════════════════════════════╗");
        println!("║              TOP DOMAINS                                 ║");
        println!("╠══════════════════════════════════════════════════════════╣");
        println!("║  Domain                    Pages  Errors  Delay (ms)     ║");
        println!("╠══════════════════════════════════════════════════════════╣");

        for stat in stats.domain_stats.iter().take(10) {
            let domain_display = if stat.domain.len() > 22 {
                format!("{}...", &stat.domain[..19])
            } else {
                stat.domain.clone()
            };

            println!(
                "║  {:22} {:>6}  {:>6}  {:>10}     ║",
                domain_display, stat.pages, stat.error_count, stat.rate_limit_delay
            );
        }

        println!("╚══════════════════════════════════════════════════════════╝");
    }
}

pub fn get_database_stats(conn: &Connection) -> Result<HashMap<String, usize>> {
    let mut stats = HashMap::new();

    let pages: usize = conn.query_row("SELECT COUNT(*) FROM pages", [], |row| row.get(0))?;
    stats.insert("pages".to_string(), pages);

    let links: usize = conn.query_row("SELECT COUNT(*) FROM links", [], |row| row.get(0))?;
    stats.insert("links".to_string(), links);

    let queue: usize = conn.query_row("SELECT COUNT(*) FROM crawl_queue", [], |row| row.get(0))?;
    stats.insert("queue".to_string(), queue);

    let domains: usize =
        conn.query_row("SELECT COUNT(*) FROM crawl_state", [], |row| row.get(0))?;
    stats.insert("domains".to_string(), domains);

    Ok(stats)
}
