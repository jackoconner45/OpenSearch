# OpenSearch

**Experimental** full-text and vector search engine with web crawler and modern UI.

## Status

⚠️ **Experimental** - Active development, APIs may change.

## Features

- Full-text search (BM25 + PageRank)
- Vector search (semantic similarity)
- Hybrid search (BM25 + vector reranking)
- Web crawler with robots.txt support
- Real-time search suggestions (WebSocket)
- Result caching and query expansion

## Architecture

**Backend (Rust)**
- `crawler` - Web crawler with rate limiting
- `parser` - HTML parser and content extraction
- `indexer` - Inverted index builder
- `embedder` - Generate embeddings (Ollama/NVIDIA/Cloudflare)
- `api_server` - REST API + WebSocket server

**Frontend (SvelteKit)**
- Settings page (search mode, results limit)
- Keyboard navigation

## Requirements

- Rust 1.70+
- Node.js 18+
- Ollama (for local embeddings) or cloud API keys

## Quick Start

### 1. Clone and setup

```bash
git clone <repo>
cd search
cp .env.example .env
```

### 2. Crawl and index

```bash
# Crawl websites
cargo run --bin crawler -- --seed https://example.com --max-pages 100

# Parse HTML
cargo run --bin parser

# Build index (with PageRank)
cargo run --bin indexer -- --pagerank

# Generate embeddings (optional, for vector search)
cargo run --bin embedder
```

### 3. Start backend

```bash
# Full-text search (default)
cargo run --bin api_server

# Vector search
cargo run --bin api_server -- -v

# Hybrid search
cargo run --bin api_server -- --hybrid
```

### 4. Start frontend

```bash
cd frontend
npm install
npm run dev
```

Open http://localhost:5173

## Configuration

### Backend (.env)

```env
API_HOST=0.0.0.0
API_PORT=5050

# For cloud embeddings
NVIDIA_API_KEY=your_key
CLOUDFLARE_API_TOKEN=your_token
CLOUDFLARE_ACCOUNT_ID=your_id
```

### Frontend (.env)

```env
PUBLIC_API_URL=http://localhost:5050
PUBLIC_WS_URL=ws://localhost:5050
```

## Keyboard Shortcuts

- `/` or `Ctrl+K` - Focus search
- `Esc` - Clear search
- `↑/↓` - Navigate suggestions
- `←/→` - Previous/next page
- `j/k` - Scroll down/up

## API

### Search
```bash
GET /search?q=query&limit=10
```

### Suggestions (WebSocket)
```javascript
ws://localhost:5050/suggest
// Send: "query prefix"
// Receive: {"prefix": "...", "suggestions": [...]}
```

## Project Structure

```
search/
├── src/
│   ├── bin/          # Binaries
│   ├── lib.rs        # Library modules
│   ├── indexer.rs    # Inverted index
│   ├── vector_index.rs
│   ├── crawler.rs
│   ├── parser.rs
│   └── ...
├── frontend/
│   └── src/routes/   # SvelteKit pages
├── Cargo.toml
└── README.md
```

## Performance

- Full-text: ~50ms per query
- Vector: ~2-3s per query (with embedding)
- Hybrid: ~1-2s per query

## Limitations

- No distributed indexing
- No incremental updates (requires full reindex)
- WebSocket suggestions require persistent connection

## Development

```bash
# Run tests
cargo test

# Build release
cargo build --release

# Frontend dev
cd frontend && npm run dev
```

## Contributing

Experimental project - contributions welcome but expect breaking changes.
