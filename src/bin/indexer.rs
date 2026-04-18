use anyhow::Result;
use clap::Parser;
use rusqlite::Connection;
use search_engine::{indexer::InvertedIndex, pagerank::LinkGraph, parser::ParsedPage};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::time::Instant;
use tracing::info;

#[derive(Parser)]
#[command(name = "indexer")]
#[command(about = "Build search index from parsed JSONL")]
struct Args {
    #[arg(long, default_value = "parsed.jsonl")]
    input: String,

    #[arg(long, default_value = "search.idx")]
    output: String,

    #[arg(long)]
    limit: Option<usize>,

    /// Compute PageRank before indexing
    #[arg(long)]
    pagerank: bool,

    /// Incremental indexing mode
    #[arg(long)]
    incremental: bool,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let args = Args::parse();
    let start_time = Instant::now();

    info!("Starting indexer");
    info!("  Input: {}", args.input);
    info!("  Output: {}", args.output);

    // Compute PageRank if requested
    if args.pagerank {
        compute_pagerank()?;
    }

    // Load PageRank scores
    info!("Loading PageRank scores from crawl.db...");
    let pr_start = Instant::now();
    let conn = Connection::open("crawl.db")?;
    let mut stmt = conn.prepare("SELECT url, score FROM pagerank")?;
    let pagerank_scores: HashMap<String, f64> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<_, _>>()?;
    info!(
        "✓ Loaded {} PageRank scores in {:.3}s",
        pagerank_scores.len(),
        pr_start.elapsed().as_secs_f64()
    );

    // Build index
    info!("Building index...");
    let build_start = Instant::now();

    let file = File::open(&args.input)?;
    let reader = BufReader::new(file);

    let mut index = InvertedIndex::new();
    let mut count = 0;

    for line in reader.lines() {
        let line = line?;
        let page: ParsedPage = serde_json::from_str(&line)?;

        // Get PageRank score (default to small value if not found)
        let pagerank = pagerank_scores.get(&page.url).copied().unwrap_or(1e-6);

        index.add_document(
            page.url.clone(),
            page.title.clone(),
            page.content,
            page.headings,
            page.word_count,
            pagerank,
        );

        count += 1;
        if count % 1000 == 0 {
            info!("  Indexed {} documents...", count);
        }

        if let Some(limit) = args.limit {
            if count >= limit {
                break;
            }
        }
    }

    info!("Finalizing index...");
    index.finalize();

    let stats = index.stats();
    info!("Index built in {:.2}s", build_start.elapsed().as_secs_f64());
    info!("  Documents: {}", stats.num_documents);
    info!("  Unique terms: {}", stats.num_terms);
    info!("  Avg doc length: {:.1} tokens", stats.avg_doc_length);

    // Save index
    info!("Saving index to {}...", args.output);
    let save_start = Instant::now();
    index.save(&args.output)?;
    info!("Index saved in {:.2}s", save_start.elapsed().as_secs_f64());

    // Check file size
    let metadata = std::fs::metadata(&args.output)?;
    let size_mb = metadata.len() as f64 / 1024.0 / 1024.0;

    info!("╔══════════════════════════════════════════════════════════╗");
    info!("║              INDEXING COMPLETE                           ║");
    info!("╠══════════════════════════════════════════════════════════╣");
    info!(
        "║  Documents indexed:  {:>8}                          ║",
        stats.num_documents
    );
    info!(
        "║  Unique terms:       {:>8}                          ║",
        stats.num_terms
    );
    info!(
        "║  Index size:         {:>8.1} MB                      ║",
        size_mb
    );
    info!(
        "║  Build time:         {:>8.1}s                        ║",
        build_start.elapsed().as_secs_f64()
    );
    info!(
        "║  Total time:         {:>8.1}s                        ║",
        start_time.elapsed().as_secs_f64()
    );
    info!("║  Output file:        {}                ║", args.output);
    info!("╚══════════════════════════════════════════════════════════╝");

    Ok(())
}

fn compute_pagerank() -> Result<()> {
    info!("Computing PageRank...");
    let start = Instant::now();
    let graph = LinkGraph::load_from_db("crawl.db")?;
    info!(
        "✓ Loaded {} URLs, {} links",
        graph.all_urls.len(),
        graph.outgoing.values().map(|v| v.len()).sum::<usize>()
    );

    let ranks = graph.compute_pagerank(0.85, 20);
    info!("✓ Computed in {:.3}s", start.elapsed().as_secs_f64());

    let conn = Connection::open("crawl.db")?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS pagerank (url TEXT PRIMARY KEY, score REAL NOT NULL)",
        [],
    )?;
    conn.execute("DELETE FROM pagerank", [])?;

    let tx = conn.unchecked_transaction()?;
    {
        let mut stmt = tx.prepare("INSERT INTO pagerank (url, score) VALUES (?, ?)")?;
        for (url, score) in &ranks {
            stmt.execute([url, &score.to_string()])?;
        }
    }
    tx.commit()?;

    conn.execute("CREATE INDEX IF NOT EXISTS idx_pagerank_url ON pagerank(url)", [])?;
    info!("✓ Stored {} PageRank scores", ranks.len());

    Ok(())
}
