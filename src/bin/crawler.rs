use anyhow::Result;
use clap::Parser;
use search_engine::parallel::ParallelCrawler;
use std::fs;
use tracing::info;

#[derive(Parser)]
#[command(name = "crawler")]
#[command(about = "Web crawler for search engine")]
struct Args {
    #[arg(long, default_value = "100000")]
    max_pages: usize,

    #[arg(long, default_value = "crawl.db")]
    db_path: String,

    #[arg(long, default_value = "urls.txt")]
    seed_file: String,

    #[arg(long, default_value = "100")]
    workers: usize,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    info!("Initializing crawler");
    info!("  Max pages: {}", args.max_pages);
    info!("  Workers: {}", args.workers);
    info!("  Database: {}", args.db_path);

    // Read seed URLs from file
    let seed_urls = read_seed_urls(&args.seed_file)?;
    info!(
        "  Loaded {} seed URLs from {}",
        seed_urls.len(),
        args.seed_file
    );

    if seed_urls.is_empty() {
        eprintln!("Error: No seed URLs found in {}", args.seed_file);
        return Ok(());
    }

    let crawler = ParallelCrawler::new(args.workers, args.max_pages);
    crawler.run(&args.db_path, seed_urls).await?;

    Ok(())
}

fn read_seed_urls(path: &str) -> Result<Vec<String>> {
    let content = fs::read_to_string(path)?;
    let urls: Vec<String> = content
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| line.to_string())
        .collect();
    Ok(urls)
}
