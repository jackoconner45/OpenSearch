use anyhow::Result;
use clap::Parser;
use search_engine::{
    analytics::Analytics,
    embeddings,
    indexer::InvertedIndex,
    vector_index::{cosine_similarity, VectorIndex},
    result_cache::ResultCache,
    query_expansion::QueryExpander,
};
use std::collections::HashMap;
use std::io::{self, Write};
use std::time::Instant;

#[derive(Parser)]
#[command(name = "search")]
#[command(about = "Interactive search interface")]
struct Args {
    #[arg(long, default_value = "search.idx")]
    index: String,

    #[arg(long, default_value = "embeddings.bin")]
    embeddings: String,

    #[arg(long, short = 'n', default_value = "10")]
    results: usize,

    /// Use vector search mode
    #[arg(short = 'v', long)]
    vector: bool,

    /// Use hybrid search mode (BM25 + vector reranking)
    #[arg(long)]
    hybrid: bool,

    /// Show search suggestions for prefix
    #[arg(long)]
    suggest: bool,

    /// Filter by domain (substring match)
    #[arg(long)]
    domain: Option<String>,

    /// Filter results after this timestamp (Unix epoch)
    #[arg(long)]
    after: Option<u64>,

    /// Filter results before this timestamp (Unix epoch)
    #[arg(long)]
    before: Option<u64>,
    
    /// Enable result caching (7-day TTL)
    #[arg(long)]
    cache: bool,
    
    /// Enable query expansion with synonyms
    #[arg(long)]
    expand: bool,

    /// Disable analytics logging
    #[arg(long)]
    no_analytics: bool,

    /// Query (if not provided, enters interactive mode)
    query: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize analytics
    let analytics = if !args.no_analytics {
        Some(Analytics::new("analytics.db")?)
    } else {
        None
    };
    
    // Initialize result cache
    let mut cache = if args.cache {
        Some(ResultCache::new(1000))
    } else {
        None
    };
    
    // Initialize query expander
    let expander = if args.expand {
        Some(QueryExpander::new())
    } else {
        None
    };

    if args.suggest {
        // Suggestion mode
        println!("Loading index...");
        let index = InvertedIndex::load(&args.index)?;
        println!("Building trie...");
        let trie = index.build_trie();
        println!("✓ Ready\n");

        if let Some(prefix) = args.query {
            show_suggestions(&trie, &prefix);
            return Ok(());
        }

        // Interactive suggestion mode
        println!("Suggestion Mode (type 'exit' or 'quit' to exit)\n");

        loop {
            print!("suggest> ");
            io::stdout().flush()?;

            let mut prefix = String::new();
            io::stdin().read_line(&mut prefix)?;
            let prefix = prefix.trim();

            if prefix.is_empty() {
                continue;
            }

            if prefix == "exit" || prefix == "quit" {
                println!("Goodbye!");
                break;
            }

            show_suggestions(&trie, prefix);
            println!();
        }
    } else if args.hybrid {
        // Hybrid search mode
        println!("Loading indices...");
        let start = Instant::now();
        let text_index = InvertedIndex::load(&args.index)?;
        let vector_index = VectorIndex::load(&args.embeddings)?;
        println!("✓ Loaded in {:.3}s\n", start.elapsed().as_secs_f64());

        // Single query mode
        if let Some(query) = args.query {
            hybrid_search_and_display(&text_index, &vector_index, &query, args.results, &analytics)
                .await;
            return Ok(());
        }

        // Interactive mode
        println!("Hybrid Search Mode (type 'exit' or 'quit' to exit)\n");

        loop {
            print!("hybrid> ");
            io::stdout().flush()?;

            let mut query = String::new();
            io::stdin().read_line(&mut query)?;
            let query = query.trim();

            if query.is_empty() {
                continue;
            }

            if query == "exit" || query == "quit" {
                println!("Goodbye!");
                break;
            }

            hybrid_search_and_display(&text_index, &vector_index, query, args.results, &analytics)
                .await;
            println!();
        }
    } else if args.vector {
        // Vector search mode
        println!("Loading vector index from {}...", args.embeddings);
        let start = Instant::now();
        let vector_index = VectorIndex::load(&args.embeddings)?;
        println!(
            "✓ Loaded {} embeddings in {:.3}s\n",
            vector_index.embeddings.len(),
            start.elapsed().as_secs_f64()
        );

        // Also load text index for document metadata
        println!("Loading document index from {}...", args.index);
        let text_index = InvertedIndex::load(&args.index)?;
        println!("✓ Loaded\n");

        // Single query mode
        if let Some(query) = args.query {
            vector_search_and_display(&vector_index, &text_index, &query, args.results, &analytics)
                .await;
            return Ok(());
        }

        // Interactive mode
        println!("Vector Search Mode (type 'exit' or 'quit' to exit)\n");

        loop {
            print!("vector> ");
            io::stdout().flush()?;

            let mut query = String::new();
            io::stdin().read_line(&mut query)?;
            let query = query.trim();

            if query.is_empty() {
                continue;
            }

            if query == "exit" || query == "quit" {
                println!("Goodbye!");
                break;
            }

            vector_search_and_display(&vector_index, &text_index, query, args.results, &analytics)
                .await;
            println!();
        }
    } else {
        // Full-text search mode
        println!("Loading index from {}...", args.index);
        let start = Instant::now();
        let index = InvertedIndex::load(&args.index)?;
        println!("✓ Loaded in {:.3}s\n", start.elapsed().as_secs_f64());

        let stats = index.stats();
        println!(
            "Index: {} documents, {} terms\n",
            stats.num_documents, stats.num_terms
        );

        // Single query mode
        if let Some(query) = args.query {
            search_and_display(
                &index,
                &query,
                args.results,
                &analytics,
                args.domain.as_deref(),
                args.after,
                args.before,
                &mut cache,
                &expander,
            );
            return Ok(());
        }

        // Interactive mode
        println!("Interactive Search (type 'exit' or 'quit' to exit)\n");

        loop {
            print!("search> ");
            io::stdout().flush()?;

            let mut query = String::new();
            io::stdin().read_line(&mut query)?;
            let query = query.trim();

            if query.is_empty() {
                continue;
            }

            if query == "exit" || query == "quit" {
                println!("Goodbye!");
                break;
            }

            search_and_display(
                &index,
                query,
                args.results,
                &analytics,
                args.domain.as_deref(),
                args.after,
                args.before,
                &mut cache,
                &expander,
            );
            println!();
        }
    }

    Ok(())
}

async fn vector_search_and_display(
    vector_index: &VectorIndex,
    text_index: &InvertedIndex,
    query: &str,
    max_results: usize,
    analytics: &Option<Analytics>,
) {
    // Embed query
    let embed_start = Instant::now();
    let query_embedding = match embeddings::embed(query).await {
        Ok(emb) => emb,
        Err(e) => {
            println!("Error embedding query: {}", e);
            return;
        }
    };
    let embed_time = embed_start.elapsed();

    // Search
    let search_start = Instant::now();
    let results = vector_index.search(&query_embedding, max_results);
    let search_time = search_start.elapsed();

    let total_time = embed_time + search_time;
    let results_count = results.len();

    // Log query
    if let Some(ref analytics) = analytics {
        let _ = analytics.log_query(
            query,
            results_count,
            total_time.as_secs_f64() * 1000.0,
            "vector",
        );
    }

    if results.is_empty() {
        println!("No results found for '{}'", query);
        return;
    }

    println!(
        "Found {} results in {:.3}ms (embed: {:.3}ms, search: {:.3}ms):\n",
        results_count,
        total_time.as_secs_f64() * 1000.0,
        embed_time.as_secs_f64() * 1000.0,
        search_time.as_secs_f64() * 1000.0
    );

    for (i, (similarity, doc_id)) in results.iter().enumerate() {
        // Get document from text index
        if let Some(doc) = text_index.documents.get(*doc_id as usize) {
            println!("{}. [Similarity: {:.3}] {}", i + 1, similarity, doc.title);
            println!("   {}", doc.url);

            // Show snippet
            let snippet = text_index.extract_snippet(doc, query, 20);
            if !snippet.is_empty() {
                println!("   {}", snippet);
            }

            if i < results.len() - 1 {
                println!();
            }
        }
    }
}

async fn hybrid_search_and_display(
    text_index: &InvertedIndex,
    vector_index: &VectorIndex,
    query: &str,
    max_results: usize,
    analytics: &Option<Analytics>,
) {
    // Stage 1: BM25 to get top 100 candidates
    let bm25_start = Instant::now();
    let bm25_results = text_index.search(query);
    let bm25_time = bm25_start.elapsed();

    if bm25_results.is_empty() {
        println!("No results found for '{}'", query);
        return;
    }

    // Take top 100 for reranking
    let candidates: Vec<_> = bm25_results.into_iter().take(100).collect();

    // Stage 2: Embed query
    let embed_start = Instant::now();
    let query_embedding = match embeddings::embed(query).await {
        Ok(emb) => emb,
        Err(e) => {
            println!("Error embedding query: {}", e);
            return;
        }
    };
    let embed_time = embed_start.elapsed();

    // Stage 3: Rerank with vector similarity
    let rerank_start = Instant::now();

    // Build doc_id -> embedding map
    let embedding_map: HashMap<u32, &Vec<f32>> = vector_index
        .embeddings
        .iter()
        .map(|(id, emb)| (*id, emb))
        .collect();

    let mut hybrid_scores = Vec::new();
    for (bm25_score, doc) in candidates {
        // Normalize BM25 score (rough normalization)
        let norm_bm25 = (bm25_score / 20.0).min(1.0);

        // Get vector similarity if embedding exists
        let vector_sim = embedding_map
            .get(&doc.doc_id)
            .map(|emb| cosine_similarity(&query_embedding, emb))
            .unwrap_or(0.0);

        // Combine: 70% BM25, 30% vector
        let combined_score = 0.7 * norm_bm25 + 0.3 * (vector_sim as f64);

        hybrid_scores.push((combined_score, bm25_score, vector_sim, doc));
    }

    // Sort by combined score
    hybrid_scores.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    let rerank_time = rerank_start.elapsed();

    let total_time = bm25_time + embed_time + rerank_time;
    let results_count = hybrid_scores.len();

    // Log query
    if let Some(ref analytics) = analytics {
        let _ = analytics.log_query(
            query,
            results_count,
            total_time.as_secs_f64() * 1000.0,
            "hybrid",
        );
    }
    println!(
        "Found {} results in {:.3}ms (BM25: {:.3}ms, embed: {:.3}ms, rerank: {:.3}ms):\n",
        hybrid_scores.len(),
        total_time.as_secs_f64() * 1000.0,
        bm25_time.as_secs_f64() * 1000.0,
        embed_time.as_secs_f64() * 1000.0,
        rerank_time.as_secs_f64() * 1000.0
    );

    for (i, (combined, bm25, vector, doc)) in hybrid_scores.iter().take(max_results).enumerate() {
        println!(
            "{}. [Hybrid: {:.3}, BM25: {:.3}, Vector: {:.3}] {}",
            i + 1,
            combined,
            bm25,
            vector,
            doc.title
        );
        println!("   {}", doc.url);

        // Show snippet
        let snippet = text_index.extract_snippet(doc, query, 20);
        if !snippet.is_empty() {
            println!("   {}", snippet);
        }

        if i < hybrid_scores.len() - 1 && i < max_results - 1 {
            println!();
        }
    }
}

fn show_suggestions(trie: &search_engine::trie::PrefixTrie, prefix: &str) {
    let suggestions = trie.suggest(prefix, 10);

    if suggestions.is_empty() {
        println!("No suggestions found for '{}'", prefix);
        return;
    }

    println!("Suggestions for '{}':\n", prefix);
    for (i, (term, freq)) in suggestions.iter().enumerate() {
        println!("{}. {} (appears in {} documents)", i + 1, term, freq);
    }
}

fn search_and_display(
    index: &InvertedIndex,
    query: &str,
    max_results: usize,
    analytics: &Option<Analytics>,
    domain: Option<&str>,
    after: Option<u64>,
    before: Option<u64>,
    cache: &mut Option<ResultCache>,
    expander: &Option<QueryExpander>,
) {
    // Expand query if enabled
    let expanded_query = if let Some(ref exp) = expander {
        let expanded = exp.expand(query);
        if expanded != query {
            println!("Expanded query: {}", expanded);
        }
        expanded
    } else {
        query.to_string()
    };
    
    // Check cache first
    if let Some(ref mut c) = cache {
        if let Some(cached) = c.get(&expanded_query) {
            println!("✓ Cache hit! Found {} results in 0.001ms:\n", cached.doc_ids.len());
            
            for (i, (doc_id, score)) in cached.doc_ids.iter().zip(cached.scores.iter()).enumerate().take(max_results) {
                let doc = &index.documents[*doc_id as usize];
                println!("{}. [Score: {:.3}] {}", i + 1, score, doc.title);
                println!("   {}", doc.url);
                
                let snippet = index.extract_snippet(doc, query, 20);
                if !snippet.is_empty() {
                    println!("   {}", snippet);
                }
                
                if i < cached.doc_ids.len() - 1 && i < max_results - 1 {
                    println!();
                }
            }
            return;
        }
    }
    
    let start = Instant::now();
    let mut results = index.search(&expanded_query);

    // Apply filters
    results = index.filter_results(results, domain, after, before);

    let search_time = start.elapsed();
    let results_count = results.len();
    
    // Store in cache
    if let Some(ref mut c) = cache {
        let doc_ids: Vec<u32> = results.iter().map(|(_, doc)| doc.doc_id).collect();
        let scores: Vec<f64> = results.iter().map(|(score, _)| *score).collect();
        c.put(&expanded_query, doc_ids, scores);
    }

    // Log query
    if let Some(ref analytics) = analytics {
        let _ = analytics.log_query(
            query,
            results_count,
            search_time.as_secs_f64() * 1000.0,
            "full-text",
        );
    }

    if results.is_empty() {
        println!("No results found for '{}'", query);
        return;
    }

    println!(
        "Found {} results in {:.3}ms:\n",
        results.len(),
        search_time.as_secs_f64() * 1000.0
    );

    for (i, (score, doc)) in results.iter().take(max_results).enumerate() {
        println!("{}. [Score: {:.3}] {}", i + 1, score, doc.title);
        println!("   {}", doc.url);

        // Show snippet
        let snippet = index.extract_snippet(doc, query, 20);
        if !snippet.is_empty() {
            println!("   {}", snippet);
        }

        if i < results.len() - 1 && i < max_results - 1 {
            println!();
        }
    }
}
