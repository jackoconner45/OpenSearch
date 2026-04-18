use anyhow::Result;
use clap::Parser;
use search_engine::{embeddings, parser::ParsedPage, embedding_cache::EmbeddingCache, quantization};
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use tracing::info;

#[derive(Parser)]
#[command(name = "embedder")]
#[command(about = "Generate embeddings for all documents")]
struct Args {
    #[arg(long, default_value = "parsed.jsonl")]
    input: String,

    #[arg(long, default_value = "embeddings.bin")]
    output: String,

    #[arg(long)]
    limit: Option<usize>,

    #[arg(long, default_value = "16")]
    batch_size: usize,
    
    /// Enable embedding cache (LRU)
    #[arg(long)]
    cache: bool,
    
    /// Cache size (number of embeddings)
    #[arg(long, default_value = "1000")]
    cache_size: usize,
    
    /// Enable f16 quantization (50% memory savings)
    #[arg(long)]
    quantize: bool,

    /// Use NVIDIA NIM API
    #[arg(long)]
    nvidia: bool,

    /// Use Cloudflare AI API
    #[arg(long)]
    cloudflare: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    dotenv::dotenv().ok();

    let args = Args::parse();
    let start_time = std::time::Instant::now();

    // Load API credentials if using cloud APIs
    let (nvidia_key, nvidia_model) = if args.nvidia {
        let key = std::env::var("NVIDIA_API_KEY")?;
        let model =
            std::env::var("NVIDIA_MODEL").unwrap_or_else(|_| "nvidia/nv-embed-v1".to_string());
        (Some(key), Some(model))
    } else {
        (None, None)
    };

    let (cf_token, cf_account, cf_model) = if args.cloudflare {
        let token = std::env::var("CLOUDFLARE_API_TOKEN")?;
        let account = std::env::var("CLOUDFLARE_ACCOUNT_ID")?;
        let model = std::env::var("CLOUDFLARE_MODEL")
            .unwrap_or_else(|_| "@cf/baai/bge-base-en-v1.5".to_string());
        (Some(token), Some(account), Some(model))
    } else {
        (None, None, None)
    };

    let api_type = if args.nvidia {
        "NVIDIA NIM"
    } else if args.cloudflare {
        "Cloudflare AI"
    } else {
        "Ollama (local)"
    };

    info!("Starting embedder");
    info!("  Input: {}", args.input);
    info!("  Output: {}", args.output);
    info!("  Batch size: {}", args.batch_size);
    info!("  Cache: {}", if args.cache { format!("enabled ({})", args.cache_size) } else { "disabled".to_string() });
    info!("  Quantization: {}", if args.quantize { "f16 (50% memory)" } else { "f32 (full)" });
    info!("  API: {}", api_type);
    
    let mut cache = if args.cache {
        Some(EmbeddingCache::new(args.cache_size))
    } else {
        None
    };

    let file = File::open(&args.input)?;
    let reader = BufReader::new(file);

    let output_file = File::create(&args.output)?;
    let mut writer = BufWriter::new(output_file);

    let mut count = 0;
    let mut total_time = 0.0;
    let mut batch = Vec::new();
    let mut batch_ids = Vec::new();

    for line in reader.lines() {
        let line = line?;
        let page: ParsedPage = serde_json::from_str(&line)?;

        // Chunk document into smaller pieces (first 2000 chars)
        let text = format!("{} {}", page.title, page.content);
        let text = text.chars().take(2000).collect::<String>();

        batch.push(text);
        batch_ids.push(count);

        // Process batch when full
        if batch.len() >= args.batch_size {
            let embed_start = std::time::Instant::now();
            
            // Check cache first
            let mut embeddings = Vec::new();
            let mut texts_to_embed = Vec::new();
            let mut cache_indices = Vec::new();
            let mut cache_hits = 0;
            
            for (i, text) in batch.iter().enumerate() {
                if let Some(ref mut c) = cache {
                    if let Some(cached_emb) = c.get(text) {
                        embeddings.push(cached_emb);
                        cache_hits += 1;
                        continue;
                    }
                }
                texts_to_embed.push(text.as_str());
                cache_indices.push(i);
            }
            
            // Embed uncached texts
            if !texts_to_embed.is_empty() {
                let new_embeddings = if let (Some(ref key), Some(ref model)) = (&nvidia_key, &nvidia_model) {
                    embeddings::nvidia_embed_batch(&texts_to_embed, key, model).await?
                } else if let (Some(ref token), Some(ref account), Some(ref model)) =
                    (&cf_token, &cf_account, &cf_model)
                {
                    embeddings::cloudflare_embed_batch(&texts_to_embed, token, account, model).await?
                } else {
                    embeddings::embed_batch(&texts_to_embed).await?
                };
                
                // Add to cache
                if let Some(ref mut c) = cache {
                    for (text, emb) in texts_to_embed.iter().zip(new_embeddings.iter()) {
                        c.put(text, emb.clone());
                    }
                }
                
                // Merge with cached results
                for emb in new_embeddings {
                    embeddings.push(emb);
                }
            }

            let embed_time = embed_start.elapsed().as_secs_f64();
            total_time += embed_time;

            // Write embeddings
            for (doc_id, embedding) in batch_ids.iter().zip(embeddings.iter()) {
                writer.write_all(&(*doc_id as u32).to_le_bytes())?;
                
                if args.quantize {
                    // Write quantized (f16)
                    let quantized = quantization::quantize_f32_to_f16(embedding);
                    writer.write_all(&(quantized.len() as u32).to_le_bytes())?;
                    writer.write_all(&quantized)?;
                } else {
                    // Write full precision (f32)
                    writer.write_all(&(embedding.len() as u32).to_le_bytes())?;
                    for val in embedding {
                        writer.write_all(&val.to_le_bytes())?;
                    }
                }
            }

            count += batch.len();
            
            if cache_hits > 0 {
                info!("  Cache hits: {}/{}", cache_hits, batch.len());
            }

            if count % 100 == 0 || count % args.batch_size == 0 {
                let avg_time = total_time / count as f64;
                let remaining = if let Some(limit) = args.limit {
                    limit.saturating_sub(count)
                } else {
                    11659 - count
                };
                let eta = remaining as f64 * avg_time;
                info!(
                    "  Embedded {} documents (avg: {:.2}s/doc, batch: {:.2}s, ETA: {:.1}min)",
                    count,
                    avg_time,
                    embed_time,
                    eta / 60.0
                );
            }

            batch.clear();
            batch_ids.clear();
        }

        if let Some(limit) = args.limit {
            if count >= limit {
                break;
            }
        }
    }

    // Process remaining batch
    if !batch.is_empty() {
        let embed_start = std::time::Instant::now();
        let texts: Vec<&str> = batch.iter().map(|s| s.as_str()).collect();

        // Route to appropriate API
        let embeddings = if let (Some(ref key), Some(ref model)) = (&nvidia_key, &nvidia_model) {
            embeddings::nvidia_embed_batch(&texts, key, model).await?
        } else if let (Some(ref token), Some(ref account), Some(ref model)) =
            (&cf_token, &cf_account, &cf_model)
        {
            embeddings::cloudflare_embed_batch(&texts, token, account, model).await?
        } else {
            embeddings::embed_batch(&texts).await?
        };
        let embed_time = embed_start.elapsed().as_secs_f64();
        total_time += embed_time;

        for (doc_id, embedding) in batch_ids.iter().zip(embeddings.iter()) {
            writer.write_all(&(*doc_id as u32).to_le_bytes())?;
            writer.write_all(&(embedding.len() as u32).to_le_bytes())?;
            for val in embedding {
                writer.write_all(&val.to_le_bytes())?;
            }
        }

        count += batch.len();
    }

    writer.flush()?;

    info!("╔══════════════════════════════════════════════════════════╗");
    info!("║              EMBEDDING COMPLETE                          ║");
    info!("╠══════════════════════════════════════════════════════════╣");
    info!(
        "║  Documents embedded: {:>8}                          ║",
        count
    );
    info!(
        "║  Avg time/doc:       {:>8.2}s                        ║",
        total_time / count as f64
    );
    info!(
        "║  Total time:         {:>8.1}s                        ║",
        start_time.elapsed().as_secs_f64()
    );
    info!("║  Output file:        {}                ║", args.output);
    info!("╚══════════════════════════════════════════════════════════╝");

    Ok(())
}
