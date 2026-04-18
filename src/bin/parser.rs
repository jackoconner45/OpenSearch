use anyhow::Result;
use clap::Parser as ClapParser;
use search_engine::{db, parser::Parser};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tracing::{info, warn};

#[derive(ClapParser)]
#[command(name = "parser")]
#[command(about = "Parse crawled HTML pages to JSONL")]
struct Args {
    #[arg(long, default_value = "crawl.db")]
    db_path: String,

    #[arg(long, default_value = "parsed.jsonl")]
    output: String,

    #[arg(long)]
    limit: Option<usize>,

    #[arg(long, default_value = "8")]
    workers: usize,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let args = Args::parse();
    let start_time = Instant::now();

    info!("Starting parser");
    info!("  Database: {}", args.db_path);
    info!("  Output: {}", args.output);
    info!("  Workers: {}", args.workers);

    // Load pages
    info!("Loading pages from database...");
    let load_start = Instant::now();
    let conn = db::init_db(&args.db_path)?;

    let total: usize = conn.query_row("SELECT COUNT(*) FROM pages", [], |row| row.get(0))?;
    let to_parse = args.limit.unwrap_or(total).min(total);

    info!("  Total pages in DB: {}", total);
    info!("  Will parse: {}", to_parse);

    let mut stmt = conn.prepare(
        "SELECT url, html_content FROM pages 
         WHERE length(html_content) > 100 
         LIMIT ?1",
    )?;

    let pages: Vec<(String, String)> = stmt
        .query_map([to_parse], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;

    info!(
        "Loaded {} pages in {:.2}s",
        pages.len(),
        load_start.elapsed().as_secs_f64()
    );

    // Open output file
    let file = File::create(&args.output)?;
    let writer = Arc::new(Mutex::new(BufWriter::with_capacity(1024 * 1024, file)));

    // Shared counters
    let parsed_count = Arc::new(Mutex::new(0usize));
    let error_count = Arc::new(Mutex::new(0usize));
    let last_log = Arc::new(Mutex::new(Instant::now()));

    info!("Starting {} workers...", args.workers);
    let parse_start = Instant::now();

    // Process in parallel
    let chunk_size = (pages.len() + args.workers - 1) / args.workers;
    let mut handles = Vec::new();

    for (worker_id, chunk) in pages.chunks(chunk_size).enumerate() {
        let chunk = chunk.to_vec();
        let writer = Arc::clone(&writer);
        let parsed_count = Arc::clone(&parsed_count);
        let error_count = Arc::clone(&error_count);
        let last_log = Arc::clone(&last_log);
        let total_pages = pages.len();

        let handle = tokio::spawn(async move {
            let parser = Parser::new();
            let mut local_parsed = 0;
            let mut local_errors = 0;

            for (url, html) in chunk {
                match parser.parse(&url, &html) {
                    Ok(parsed) => {
                        match parsed.to_jsonl() {
                            Ok(jsonl) => {
                                let mut w = writer.lock().unwrap();
                                if writeln!(w, "{}", jsonl).is_ok() {
                                    drop(w);
                                    local_parsed += 1;

                                    let mut count = parsed_count.lock().unwrap();
                                    *count += 1;
                                    let current = *count;
                                    drop(count);

                                    // Log every 500 pages or every 5 seconds
                                    let mut last = last_log.lock().unwrap();
                                    if current % 500 == 0 || last.elapsed().as_secs() >= 5 {
                                        let rate =
                                            current as f64 / parse_start.elapsed().as_secs_f64();
                                        info!(
                                            "Progress: {}/{} pages ({:.1}%), {:.1} pages/sec",
                                            current,
                                            total_pages,
                                            (current as f64 / total_pages as f64) * 100.0,
                                            rate
                                        );
                                        *last = Instant::now();
                                    }
                                } else {
                                    local_errors += 1;
                                }
                            }
                            Err(e) => {
                                warn!("JSONL error for {}: {}", url, e);
                                local_errors += 1;
                            }
                        }
                    }
                    Err(e) => {
                        warn!("Parse error for {}: {}", url, e);
                        local_errors += 1;
                    }
                }
            }

            if local_errors > 0 {
                let mut count = error_count.lock().unwrap();
                *count += local_errors;
            }

            info!(
                "Worker {} finished: {} parsed, {} errors",
                worker_id, local_parsed, local_errors
            );
        });

        handles.push(handle);
    }

    // Wait for all workers
    for handle in handles {
        handle.await?;
    }

    // Flush output
    info!("Flushing output...");
    {
        let mut w = writer.lock().unwrap();
        w.flush()?;
    }

    let final_parsed = *parsed_count.lock().unwrap();
    let final_errors = *error_count.lock().unwrap();
    let total_time = start_time.elapsed();
    let parse_time = parse_start.elapsed();

    info!("╔══════════════════════════════════════════════════════════╗");
    info!("║              PARSING COMPLETE                            ║");
    info!("╠══════════════════════════════════════════════════════════╣");
    info!(
        "║  Pages parsed:       {:>8}                          ║",
        final_parsed
    );
    info!(
        "║  Errors:             {:>8}                          ║",
        final_errors
    );
    info!(
        "║  Parse time:         {:>8.1}s                        ║",
        parse_time.as_secs_f64()
    );
    info!(
        "║  Total time:         {:>8.1}s                        ║",
        total_time.as_secs_f64()
    );
    info!(
        "║  Average rate:       {:>8.1} pages/sec               ║",
        final_parsed as f64 / parse_time.as_secs_f64()
    );
    info!(
        "║  Output file:        {}                    ║",
        args.output
    );
    info!("╚══════════════════════════════════════════════════════════╝");

    Ok(())
}
