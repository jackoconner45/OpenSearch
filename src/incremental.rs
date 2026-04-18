use crate::{indexer::InvertedIndex, parser::ParsedPage};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::Path;

const DEFAULT_PAGERANK: f64 = 1e-6;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IncrementalState {
    pub source_path: String,
    pub byte_offset: u64,
    pub processed_lines: u64,
    pub total_updates: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IncrementalSummary {
    pub processed_lines: usize,
    pub new_documents: usize,
    pub updated_documents: usize,
    pub unchanged_documents: usize,
    pub final_offset: u64,
    pub total_documents: usize,
}

pub fn run_incremental_indexing(
    input_path: &str,
    index_path: &str,
    state_path: &str,
    limit: Option<usize>,
) -> Result<IncrementalSummary> {
    let mut state = load_state(state_path, input_path)?;
    let input_metadata = std::fs::metadata(input_path)?;
    let mut reset_index = false;

    if state.byte_offset > input_metadata.len() {
        state.byte_offset = 0;
        state.processed_lines = 0;
        reset_index = true;
    }

    let mut index = if !reset_index && Path::new(index_path).exists() {
        InvertedIndex::load(index_path)?
    } else {
        InvertedIndex::new()
    };

    let input = File::open(input_path)?;
    let mut reader = BufReader::new(input);
    reader.seek(SeekFrom::Start(state.byte_offset))?;

    let mut summary = IncrementalSummary::default();
    let mut line = String::new();

    loop {
        line.clear();
        let bytes_read = reader.read_line(&mut line)?;
        if bytes_read == 0 {
            break;
        }

        if let Some(limit) = limit {
            if summary.processed_lines >= limit {
                break;
            }
        }

        let trimmed = line.trim();
        state.byte_offset = reader.stream_position()?;
        if trimmed.is_empty() {
            continue;
        }

        let page: ParsedPage = serde_json::from_str(trimmed)?;
        summary.processed_lines += 1;
        state.processed_lines += 1;

        let existed = index.url_to_doc_id.contains_key(&page.url);
        if existed {
            if index.add_or_update_document(
                page.url.clone(),
                page.title,
                page.content,
                page.headings,
                page.word_count,
                DEFAULT_PAGERANK,
            ) {
                summary.updated_documents += 1;
                state.total_updates += 1;
            } else {
                summary.unchanged_documents += 1;
            }
        } else {
            index.add_document(
                page.url.clone(),
                page.title,
                page.content,
                page.headings,
                page.word_count,
                DEFAULT_PAGERANK,
            );
            summary.new_documents += 1;
        }
    }

    index.finalize();
    index.save(index_path)?;

    state.source_path = input_path.to_string();
    save_state(state_path, &state)?;

    summary.final_offset = state.byte_offset;
    summary.total_documents = index.total_docs;

    Ok(summary)
}

fn load_state(state_path: &str, input_path: &str) -> Result<IncrementalState> {
    if !Path::new(state_path).exists() {
        return Ok(IncrementalState {
            source_path: input_path.to_string(),
            ..IncrementalState::default()
        });
    }

    let raw = std::fs::read_to_string(state_path)?;
    let mut state: IncrementalState = serde_json::from_str(&raw)?;
    if state.source_path != input_path {
        state.source_path = input_path.to_string();
        state.byte_offset = 0;
        state.processed_lines = 0;
    }
    Ok(state)
}

fn save_state(state_path: &str, state: &IncrementalState) -> Result<()> {
    std::fs::write(state_path, serde_json::to_vec_pretty(state)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_path(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("search_engine_{name}_{unique}"))
    }

    fn sample_page(url: &str, title: &str, content: &str) -> String {
        serde_json::to_string(&ParsedPage {
            url: url.to_string(),
            title: title.to_string(),
            meta_description: String::new(),
            content: content.to_string(),
            content_length: content.len(),
            headings: vec![title.to_string()],
            links: Vec::new(),
            word_count: content.split_whitespace().count(),
        })
        .unwrap()
    }

    #[test]
    fn incremental_indexer_only_processes_new_bytes() {
        let input = temp_path("input.jsonl");
        let index = temp_path("index.idx");
        let state = temp_path("state.json");

        std::fs::write(
            &input,
            format!(
                "{}\n{}\n",
                sample_page("https://example.com/1", "One", "alpha beta gamma"),
                sample_page("https://example.com/2", "Two", "delta epsilon zeta")
            ),
        )
        .unwrap();

        let first = run_incremental_indexing(
            input.to_str().unwrap(),
            index.to_str().unwrap(),
            state.to_str().unwrap(),
            None,
        )
        .unwrap();
        assert_eq!(first.new_documents, 2);
        assert_eq!(first.processed_lines, 2);

        std::fs::OpenOptions::new()
            .append(true)
            .open(&input)
            .unwrap()
            .write_all(
                format!(
                    "{}\n{}\n",
                    sample_page(
                        "https://example.com/2",
                        "Two updated",
                        "delta epsilon search"
                    ),
                    sample_page("https://example.com/3", "Three", "new document content")
                )
                .as_bytes(),
            )
            .unwrap();

        let second = run_incremental_indexing(
            input.to_str().unwrap(),
            index.to_str().unwrap(),
            state.to_str().unwrap(),
            None,
        )
        .unwrap();

        assert_eq!(second.processed_lines, 2);
        assert_eq!(second.new_documents, 1);
        assert_eq!(second.updated_documents, 1);

        let loaded = InvertedIndex::load(index.to_str().unwrap()).unwrap();
        let results = loaded.search("document");
        assert!(results
            .iter()
            .any(|(_, doc)| doc.url == "https://example.com/3"));
    }

    #[test]
    fn incremental_indexer_resets_when_source_shrinks() {
        let input = temp_path("rewrite.jsonl");
        let index = temp_path("rewrite.idx");
        let state = temp_path("rewrite_state.json");

        std::fs::write(
            &input,
            format!(
                "{}\n",
                sample_page(
                    "https://example.com/1",
                    "One",
                    "alpha beta gamma delta epsilon zeta eta theta iota"
                )
            ),
        )
        .unwrap();
        run_incremental_indexing(
            input.to_str().unwrap(),
            index.to_str().unwrap(),
            state.to_str().unwrap(),
            None,
        )
        .unwrap();

        std::fs::write(
            &input,
            format!(
                "{}\n",
                sample_page("https://example.com/2", "Two", "beta gamma")
            ),
        )
        .unwrap();
        let summary = run_incremental_indexing(
            input.to_str().unwrap(),
            index.to_str().unwrap(),
            state.to_str().unwrap(),
            None,
        )
        .unwrap();

        assert_eq!(summary.processed_lines, 1);
        let loaded = InvertedIndex::load(index.to_str().unwrap()).unwrap();
        assert!(loaded
            .search("beta")
            .iter()
            .any(|(_, doc)| doc.url == "https://example.com/2"));
        assert!(!loaded
            .search("theta")
            .iter()
            .any(|(_, doc)| doc.url == "https://example.com/1"));
    }

    #[test]
    fn incremental_indexer_updates_title_only_changes() {
        let input = temp_path("title_updates.jsonl");
        let index = temp_path("title_updates.idx");
        let state = temp_path("title_updates_state.json");

        std::fs::write(
            &input,
            format!(
                "{}\n",
                sample_page(
                    "https://example.com/rust",
                    "Rust Original",
                    "rust search systems"
                )
            ),
        )
        .unwrap();

        run_incremental_indexing(
            input.to_str().unwrap(),
            index.to_str().unwrap(),
            state.to_str().unwrap(),
            None,
        )
        .unwrap();

        let original = InvertedIndex::load(index.to_str().unwrap()).unwrap();
        let original_doc_id = original.url_to_doc_id["https://example.com/rust"];

        std::fs::OpenOptions::new()
            .append(true)
            .open(&input)
            .unwrap()
            .write_all(
                format!(
                    "{}\n",
                    sample_page(
                        "https://example.com/rust",
                        "Rust Renamed",
                        "rust search systems"
                    )
                )
                .as_bytes(),
            )
            .unwrap();

        let summary = run_incremental_indexing(
            input.to_str().unwrap(),
            index.to_str().unwrap(),
            state.to_str().unwrap(),
            None,
        )
        .unwrap();

        assert_eq!(summary.updated_documents, 1);

        let loaded = InvertedIndex::load(index.to_str().unwrap()).unwrap();
        let top = loaded.search("rust").into_iter().next().unwrap().1;
        assert_eq!(top.title, "Rust Renamed");
        assert_eq!(
            loaded.url_to_doc_id["https://example.com/rust"],
            original_doc_id
        );
    }
}
