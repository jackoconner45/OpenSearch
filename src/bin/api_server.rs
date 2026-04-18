use anyhow::Result;
use axum::{
    extract::{ws::WebSocket, Query, State, WebSocketUpgrade},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::get,
    Router,
};
use clap::Parser;
use search_engine::{
    embeddings, indexer::InvertedIndex, trie::PrefixTrie, vector_index::VectorIndex,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tower_http::cors::CorsLayer;

#[derive(Parser)]
#[command(name = "api_server")]
#[command(about = "REST API server for search engine")]
struct Args {
    #[arg(long, default_value = "search.idx")]
    index: String,

    #[arg(long, default_value = "embeddings.bin")]
    embeddings: String,

    /// Use vector search mode
    #[arg(short = 'v', long)]
    vector: bool,

    /// Use hybrid search mode
    #[arg(long)]
    hybrid: bool,
}

#[derive(Clone)]
struct AppState {
    text_index: Arc<InvertedIndex>,
    vector_index: Option<Arc<VectorIndex>>,
    trie: Arc<PrefixTrie>,
    mode: SearchMode,
}

#[derive(Clone)]
enum SearchMode {
    FullText,
    Vector,
    Hybrid,
}

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
    #[serde(default = "default_limit")]
    limit: usize,
}

fn default_limit() -> usize {
    10
}

#[derive(Serialize)]
struct SearchResponse {
    query: String,
    results: Vec<SearchResult>,
    total: usize,
    time_ms: f64,
}

#[derive(Serialize)]
struct SearchResult {
    title: String,
    url: String,
    snippet: String,
    score: f64,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    dotenv::dotenv().ok();
    let args = Args::parse();

    let host = std::env::var("API_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    let port = std::env::var("API_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(5050);

    println!("Loading indices...");
    let text_index = Arc::new(InvertedIndex::load(&args.index)?);
    
    println!("Building trie for suggestions...");
    let trie = Arc::new(text_index.build_trie());
    
    let vector_index = if args.vector || args.hybrid {
        Some(Arc::new(VectorIndex::load(&args.embeddings)?))
    } else {
        None
    };

    let mode = if args.hybrid {
        SearchMode::Hybrid
    } else if args.vector {
        SearchMode::Vector
    } else {
        SearchMode::FullText
    };

    let mode_str = match mode {
        SearchMode::FullText => "full-text",
        SearchMode::Vector => "vector",
        SearchMode::Hybrid => "hybrid",
    };

    println!("✓ Loaded indices");
    println!("Mode: {}", mode_str);

    let state = AppState {
        text_index,
        vector_index,
        trie,
        mode,
    };

    let app = Router::new()
        .route("/search", get(search_handler))
        .route("/suggest", get(suggest_ws_handler))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = format!("{}:{}", host, port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    println!("🚀 Server running on http://{}", addr);
    println!("Try: curl 'http://localhost:{}/search?q=rust&limit=5'", port);
    println!("WebSocket: ws://localhost:{}/suggest", port);

    axum::serve(listener, app).await?;
    Ok(())
}

async fn suggest_ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_suggest_socket(socket, state))
}

async fn handle_suggest_socket(mut socket: WebSocket, state: AppState) {
    use axum::extract::ws::Message;

    while let Some(Ok(msg)) = socket.recv().await {
        if let Message::Text(prefix) = msg {
            let prefix = prefix.trim();
            if prefix.is_empty() {
                continue;
            }

            let suggestions = state.trie.suggest(prefix, 10);
            let response = serde_json::json!({
                "prefix": prefix,
                "suggestions": suggestions.iter().map(|(term, freq)| {
                    serde_json::json!({
                        "term": term,
                        "frequency": freq
                    })
                }).collect::<Vec<_>>()
            });

            if socket.send(Message::Text(response.to_string())).await.is_err() {
                break;
            }
        }
    }
}

async fn search_handler(
    State(state): State<AppState>,
    Query(params): Query<SearchQuery>,
) -> Result<Json<SearchResponse>, (StatusCode, String)> {
    let start = std::time::Instant::now();
    let query = params.q.trim();

    if query.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "Query parameter 'q' is required".to_string()));
    }

    let results = match state.mode {
        SearchMode::FullText => {
            let search_results = state.text_index.search(query);
            search_results
                .into_iter()
                .take(params.limit)
                .map(|(score, doc)| SearchResult {
                    title: doc.title.clone(),
                    url: doc.url.clone(),
                    snippet: state.text_index.extract_snippet(doc, query, 20),
                    score,
                })
                .collect()
        }
        SearchMode::Vector => {
            let vector_index = state.vector_index.as_ref().unwrap();
            let query_embedding = embeddings::embed(query)
                .await
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

            let search_results = vector_index.search(&query_embedding, params.limit);
            search_results
                .into_iter()
                .filter_map(|(similarity, doc_id)| {
                    state.text_index.documents.get(doc_id as usize).map(|doc| SearchResult {
                        title: doc.title.clone(),
                        url: doc.url.clone(),
                        snippet: state.text_index.extract_snippet(doc, query, 20),
                        score: similarity as f64,
                    })
                })
                .collect()
        }
        SearchMode::Hybrid => {
            let vector_index = state.vector_index.as_ref().unwrap();
            
            // Stage 1: BM25
            let bm25_results = state.text_index.search(query);
            let candidates: Vec<_> = bm25_results.into_iter().take(100).collect();

            if candidates.is_empty() {
                vec![]
            } else {
                // Stage 2: Embed query
                let query_embedding = embeddings::embed(query)
                    .await
                    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

                // Stage 3: Rerank
                let embedding_map: HashMap<u32, &Vec<f32>> = vector_index
                    .embeddings
                    .iter()
                    .map(|(id, emb)| (*id, emb))
                    .collect();

                let mut hybrid_scores = Vec::new();
                for (bm25_score, doc) in candidates {
                    let norm_bm25 = (bm25_score / 20.0).min(1.0);
                    let vector_sim = embedding_map
                        .get(&doc.doc_id)
                        .map(|emb| search_engine::vector_index::cosine_similarity(&query_embedding, emb))
                        .unwrap_or(0.0);
                    let combined_score = 0.7 * norm_bm25 + 0.3 * (vector_sim as f64);

                    hybrid_scores.push((combined_score, doc));
                }

                hybrid_scores.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
                hybrid_scores
                    .into_iter()
                    .take(params.limit)
                    .map(|(score, doc)| SearchResult {
                        title: doc.title.clone(),
                        url: doc.url.clone(),
                        snippet: state.text_index.extract_snippet(doc, query, 20),
                        score,
                    })
                    .collect()
            }
        }
    };

    let time_ms = start.elapsed().as_secs_f64() * 1000.0;
    let total = results.len();

    Ok(Json(SearchResponse {
        query: query.to_string(),
        results,
        total,
        time_ms,
    }))
}
