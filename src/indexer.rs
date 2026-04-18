use anyhow::Result;
use rust_stemmers::{Algorithm, Stemmer};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, BufWriter};

// Field boost multipliers
const TITLE_BOOST: f64 = 3.0;
const HEADING_BOOST: f64 = 2.0;
const URL_BOOST: f64 = 1.5;

// Fuzzy matching threshold
const MAX_EDIT_DISTANCE: usize = 2;

// Common English stop words
const STOP_WORDS: &[&str] = &[
    "the", "and", "for", "are", "but", "not", "you", "all", "can", "her", "was", "one", "our",
    "out", "day", "get", "has", "him", "his", "how", "man", "new", "now", "old", "see", "two",
    "way", "who", "boy", "did", "its", "let", "put", "say", "she", "too", "use",
];

#[derive(Debug, Clone)]
pub enum QueryTerm {
    Required(String),    // Must have
    Optional(String),    // OR
    Excluded(String),    // NOT
    Phrase(Vec<String>), // "exact phrase"
}

pub fn parse_query(query: &str) -> Vec<QueryTerm> {
    let mut terms = Vec::new();
    let mut chars = query.chars().peekable();
    let mut current = String::new();
    let mut in_phrase = false;
    let mut phrase_words = Vec::new();
    let mut is_excluded = false;

    while let Some(ch) = chars.next() {
        match ch {
            '"' => {
                if in_phrase {
                    // End phrase
                    if !phrase_words.is_empty() {
                        terms.push(QueryTerm::Phrase(phrase_words.clone()));
                        phrase_words.clear();
                    }
                    in_phrase = false;
                } else {
                    // Start phrase
                    if !current.is_empty() {
                        let term = current.trim().to_string();
                        if !term.is_empty() {
                            terms.push(if is_excluded {
                                QueryTerm::Excluded(term)
                            } else {
                                QueryTerm::Required(term)
                            });
                        }
                        current.clear();
                        is_excluded = false;
                    }
                    in_phrase = true;
                }
            }
            ' ' if !in_phrase => {
                let word = current.trim();
                if !word.is_empty() {
                    if word.eq_ignore_ascii_case("OR") {
                        // Previous term becomes optional
                        if let Some(last) = terms.last_mut() {
                            if let QueryTerm::Required(term) = last {
                                *last = QueryTerm::Optional(term.clone());
                            }
                        }
                    } else if word.eq_ignore_ascii_case("AND") {
                        // Skip, default behavior
                    } else if word.eq_ignore_ascii_case("NOT") || word == "-" {
                        is_excluded = true;
                    } else {
                        terms.push(if is_excluded {
                            QueryTerm::Excluded(word.to_string())
                        } else {
                            QueryTerm::Required(word.to_string())
                        });
                        is_excluded = false;
                    }
                }
                current.clear();
            }
            ' ' if in_phrase => {
                if !current.is_empty() {
                    phrase_words.push(current.clone());
                    current.clear();
                }
            }
            '-' if current.is_empty() && !in_phrase => {
                is_excluded = true;
            }
            _ => {
                current.push(ch);
            }
        }
    }

    // Handle remaining
    if in_phrase && !phrase_words.is_empty() {
        terms.push(QueryTerm::Phrase(phrase_words));
    } else if !current.is_empty() {
        let word = current.trim();
        if !word.is_empty() && !word.eq_ignore_ascii_case("OR") && !word.eq_ignore_ascii_case("AND")
        {
            terms.push(if is_excluded {
                QueryTerm::Excluded(word.to_string())
            } else {
                QueryTerm::Required(word.to_string())
            });
        }
    }

    terms
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TermFreq {
    pub title: u32,
    pub headings: u32,
    pub url: u32,
    pub body: u32,
}

impl TermFreq {
    fn new() -> Self {
        Self {
            title: 0,
            headings: 0,
            url: 0,
            body: 0,
        }
    }

    fn boosted_freq(&self) -> f64 {
        (self.title as f64 * TITLE_BOOST)
            + (self.headings as f64 * HEADING_BOOST)
            + (self.url as f64 * URL_BOOST)
            + (self.body as f64)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Document {
    pub doc_id: u32,
    pub url: String,
    pub title: String,
    pub word_count: usize,
    pub content: String,
    pub pagerank: f64,
    pub content_hash: String,
    pub timestamp: u64,
    pub domain: String,
    pub content_type: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InvertedIndex {
    pub index: HashMap<String, Vec<(u32, TermFreq)>>,
    pub documents: Vec<Document>,
    pub doc_lengths: Vec<usize>,
    pub avg_doc_length: f64,
    pub total_docs: usize,
    pub url_to_doc_id: HashMap<String, u32>,
}

struct PreparedDocument {
    title: String,
    word_count: usize,
    content: String,
    pagerank: f64,
    content_hash: String,
    timestamp: u64,
    domain: String,
    content_type: String,
    doc_length: usize,
    term_freqs: HashMap<String, TermFreq>,
}

impl PreparedDocument {
    fn document(&self, doc_id: u32, url: String) -> Document {
        Document {
            doc_id,
            url,
            title: self.title.clone(),
            word_count: self.word_count,
            content: self.content.clone(),
            pagerank: self.pagerank,
            content_hash: self.content_hash.clone(),
            timestamp: self.timestamp,
            domain: self.domain.clone(),
            content_type: self.content_type.clone(),
        }
    }
}

impl InvertedIndex {
    pub fn new() -> Self {
        Self {
            index: HashMap::new(),
            documents: Vec::new(),
            doc_lengths: Vec::new(),
            avg_doc_length: 0.0,
            total_docs: 0,
            url_to_doc_id: HashMap::new(),
        }
    }

    pub fn add_document(
        &mut self,
        url: String,
        title: String,
        content: String,
        headings: Vec<String>,
        word_count: usize,
        pagerank: f64,
    ) {
        let doc_id = self.documents.len() as u32;
        self.insert_new_document(doc_id, url, title, content, headings, word_count, pagerank);
        self.total_docs += 1;
    }

    pub fn add_or_update_document(
        &mut self,
        url: String,
        title: String,
        content: String,
        headings: Vec<String>,
        word_count: usize,
        pagerank: f64,
    ) -> bool {
        let new_hash = Self::compute_indexed_content_hash(&title, &content, &headings);

        if let Some(&existing_doc_id) = self.url_to_doc_id.get(&url) {
            let existing_doc = &self.documents[existing_doc_id as usize];
            if existing_doc.content_hash == new_hash {
                return false;
            }

            self.remove_postings_for_doc(existing_doc_id);
            self.update_existing_document(
                existing_doc_id,
                url,
                title,
                content,
                headings,
                word_count,
                pagerank,
                new_hash,
            );
            return true;
        }

        let doc_id = self.documents.len() as u32;
        self.insert_new_document(doc_id, url, title, content, headings, word_count, pagerank);
        self.total_docs += 1;
        true
    }

    fn insert_new_document(
        &mut self,
        doc_id: u32,
        url: String,
        title: String,
        content: String,
        headings: Vec<String>,
        word_count: usize,
        pagerank: f64,
    ) {
        let prepared =
            Self::prepare_document_fields(&url, &title, &content, &headings, word_count, pagerank);
        self.insert_postings(doc_id, &prepared.term_freqs);
        self.documents.push(prepared.document(doc_id, url.clone()));
        self.doc_lengths.push(prepared.doc_length);
        self.url_to_doc_id.insert(url, doc_id);
    }

    fn update_existing_document(
        &mut self,
        doc_id: u32,
        url: String,
        title: String,
        content: String,
        headings: Vec<String>,
        word_count: usize,
        pagerank: f64,
        content_hash: String,
    ) {
        let prepared = Self::prepare_document_fields_with_hash(
            &url,
            &title,
            &content,
            &headings,
            word_count,
            pagerank,
            content_hash,
        );

        self.insert_postings(doc_id, &prepared.term_freqs);
        self.documents[doc_id as usize] = prepared.document(doc_id, url.clone());
        self.doc_lengths[doc_id as usize] = prepared.doc_length;
        self.url_to_doc_id.insert(url, doc_id);
    }

    fn insert_postings(&mut self, doc_id: u32, term_freqs: &HashMap<String, TermFreq>) {
        for (term, freq) in term_freqs {
            self.index
                .entry(term.clone())
                .or_insert_with(Vec::new)
                .push((doc_id, freq.clone()));
        }
    }

    fn remove_postings_for_doc(&mut self, doc_id: u32) {
        for postings in self.index.values_mut() {
            postings.retain(|(id, _)| *id != doc_id);
        }
        self.index.retain(|_, postings| !postings.is_empty());
    }

    fn prepare_document_fields(
        url: &str,
        title: &str,
        content: &str,
        headings: &[String],
        word_count: usize,
        pagerank: f64,
    ) -> PreparedDocument {
        let content_hash = Self::compute_indexed_content_hash(title, content, headings);
        Self::prepare_document_fields_with_hash(
            url,
            title,
            content,
            headings,
            word_count,
            pagerank,
            content_hash,
        )
    }

    fn prepare_document_fields_with_hash(
        url: &str,
        title: &str,
        content: &str,
        headings: &[String],
        word_count: usize,
        pagerank: f64,
        content_hash: String,
    ) -> PreparedDocument {
        let title_tokens = tokenize(title);
        let heading_tokens: Vec<String> = headings
            .iter()
            .flat_map(|heading| tokenize(heading))
            .collect();
        let url_tokens = tokenize(url);
        let body_tokens = tokenize(content);
        let doc_length = body_tokens.len();
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut term_freqs: HashMap<String, TermFreq> = HashMap::new();
        for token in title_tokens {
            term_freqs.entry(token).or_insert_with(TermFreq::new).title += 1;
        }
        for token in heading_tokens {
            term_freqs
                .entry(token)
                .or_insert_with(TermFreq::new)
                .headings += 1;
        }
        for token in url_tokens {
            term_freqs.entry(token).or_insert_with(TermFreq::new).url += 1;
        }
        for token in body_tokens {
            term_freqs.entry(token).or_insert_with(TermFreq::new).body += 1;
        }

        PreparedDocument {
            title: title.to_string(),
            word_count,
            content: content.to_string(),
            pagerank,
            content_hash,
            timestamp,
            domain: Self::extract_domain(url),
            content_type: Self::detect_content_type(url, title),
            doc_length,
            term_freqs,
        }
    }

    fn compute_indexed_content_hash(title: &str, content: &str, headings: &[String]) -> String {
        use sha2::{Digest, Sha256};

        let mut hasher = Sha256::new();
        hasher.update(title.as_bytes());
        hasher.update([0]);
        for heading in headings {
            hasher.update(heading.as_bytes());
            hasher.update([0]);
        }
        hasher.update(content.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    pub fn remove_document(&mut self, url: &str) -> bool {
        // Find document ID
        let doc_id = match self.url_to_doc_id.get(url) {
            Some(&id) => id,
            None => return false, // Document not found
        };

        // Remove from inverted index
        for postings in self.index.values_mut() {
            postings.retain(|(id, _)| *id != doc_id);
        }

        // Clean up empty posting lists
        self.index.retain(|_, postings| !postings.is_empty());

        // Mark document as deleted (set empty content)
        if let Some(doc) = self.documents.get_mut(doc_id as usize) {
            doc.content = String::new();
            doc.title = String::from("[DELETED]");
        }
        if let Some(length) = self.doc_lengths.get_mut(doc_id as usize) {
            *length = 0;
        }

        // Remove from URL mapping
        self.url_to_doc_id.remove(url);

        // Update total docs
        self.total_docs = self.total_docs.saturating_sub(1);

        true
    }

    pub fn finalize(&mut self) {
        let active_lengths: Vec<usize> = self
            .url_to_doc_id
            .values()
            .filter_map(|doc_id| self.doc_lengths.get(*doc_id as usize).copied())
            .filter(|length| *length > 0)
            .collect();

        self.avg_doc_length = if active_lengths.is_empty() {
            0.0
        } else {
            active_lengths.iter().sum::<usize>() as f64 / active_lengths.len() as f64
        };
    }

    pub fn save(&self, path: &str) -> Result<()> {
        let file = File::create(path)?;
        let writer = BufWriter::new(file);
        bincode::serialize_into(writer, self)?;
        Ok(())
    }

    pub fn load(path: &str) -> Result<Self> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let index = bincode::deserialize_from(reader)?;
        Ok(index)
    }

    pub fn search_bm25(&self, query: &str, k1: f64, b: f64) -> Vec<(f64, &Document)> {
        let parsed_query = parse_query(query);
        let mut doc_scores: HashMap<u32, f64> = HashMap::new();
        let mut excluded_docs: std::collections::HashSet<u32> = std::collections::HashSet::new();
        let mut required_docs: Option<std::collections::HashSet<u32>> = None;

        for query_term in parsed_query {
            match &query_term {
                QueryTerm::Required(term) | QueryTerm::Optional(term) => {
                    let is_required = matches!(query_term, QueryTerm::Required(_));
                    let tokens = tokenize(term);
                    for token in tokens {
                        // Try exact match first
                        let mut found_exact = false;
                        if let Some(postings) = self.index.get(&token) {
                            found_exact = true;
                            let df = postings.len() as f64;
                            let idf = ((self.total_docs as f64 - df + 0.5) / (df + 0.5) + 1.0).ln();

                            let mut term_docs = std::collections::HashSet::new();
                            for (doc_id, term_freq) in postings {
                                term_docs.insert(*doc_id);
                                let doc_len = self.doc_lengths[*doc_id as usize] as f64;
                                let tf = term_freq.boosted_freq();
                                let norm = 1.0 - b + b * (doc_len / self.avg_doc_length);
                                let bm25_score = idf * (tf * (k1 + 1.0)) / (tf + k1 * norm);

                                // Apply PageRank boost (log scale to avoid dominating)
                                let pagerank = self.documents[*doc_id as usize].pagerank;
                                let pagerank_boost = 1.0 + (pagerank * 10000.0).ln().max(0.0);
                                let final_score = bm25_score * pagerank_boost;

                                *doc_scores.entry(*doc_id).or_insert(0.0) += final_score;
                            }

                            // Track required terms
                            if is_required {
                                if let Some(ref mut req) = required_docs {
                                    *req = req.intersection(&term_docs).copied().collect();
                                } else {
                                    required_docs = Some(term_docs);
                                }
                            }
                        }

                        // Fuzzy matching if no exact match
                        if !found_exact && token.len() >= 4 {
                            let mut fuzzy_matches = Vec::new();
                            let token_len = token.len();

                            for index_term in self.index.keys() {
                                // Quick length filter
                                let len_diff =
                                    (index_term.len() as i32 - token_len as i32).abs() as usize;
                                if len_diff > MAX_EDIT_DISTANCE {
                                    continue;
                                }

                                let dist = levenshtein_distance(&token, index_term);
                                if dist <= MAX_EDIT_DISTANCE && dist > 0 {
                                    fuzzy_matches.push((index_term, dist));
                                }
                            }

                            // Limit to top 5 closest matches
                            fuzzy_matches.sort_by_key(|(_, dist)| *dist);
                            fuzzy_matches.truncate(5);

                            // Use closest matches with penalty
                            for (fuzzy_term, dist) in fuzzy_matches {
                                if let Some(postings) = self.index.get(fuzzy_term) {
                                    let df = postings.len() as f64;
                                    let idf = ((self.total_docs as f64 - df + 0.5) / (df + 0.5)
                                        + 1.0)
                                        .ln();
                                    let penalty = 0.5_f64.powi(dist as i32); // Reduce score based on distance

                                    for (doc_id, term_freq) in postings {
                                        let doc_len = self.doc_lengths[*doc_id as usize] as f64;
                                        let tf = term_freq.boosted_freq();
                                        let norm = 1.0 - b + b * (doc_len / self.avg_doc_length);
                                        let score =
                                            idf * (tf * (k1 + 1.0)) / (tf + k1 * norm) * penalty;
                                        *doc_scores.entry(*doc_id).or_insert(0.0) += score;
                                    }
                                }
                            }
                        }
                    }
                }
                QueryTerm::Excluded(term) => {
                    let tokens = tokenize(term);
                    for token in tokens {
                        if let Some(postings) = self.index.get(&token) {
                            for (doc_id, _) in postings {
                                excluded_docs.insert(*doc_id);
                            }
                        }
                    }
                }
                QueryTerm::Phrase(_words) => {
                    // Simplified: treat as required terms for now
                    // Full phrase matching would require position tracking
                }
            }
        }

        // Filter results
        let mut results: Vec<(u32, f64)> = doc_scores
            .into_iter()
            .filter(|(doc_id, _)| !excluded_docs.contains(doc_id))
            .filter(|(doc_id, _)| {
                if let Some(ref req) = required_docs {
                    req.contains(doc_id)
                } else {
                    true
                }
            })
            .collect();

        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        results
            .into_iter()
            .take(20)
            .map(|(doc_id, score)| (score, &self.documents[doc_id as usize]))
            .collect()
    }

    pub fn filter_results<'a>(
        &'a self,
        results: Vec<(f64, &'a Document)>,
        domain: Option<&str>,
        after: Option<u64>,
        before: Option<u64>,
    ) -> Vec<(f64, &'a Document)> {
        results
            .into_iter()
            .filter(|(_, doc)| {
                if let Some(d) = domain {
                    if !doc.domain.contains(d) {
                        return false;
                    }
                }
                if let Some(a) = after {
                    if doc.timestamp < a {
                        return false;
                    }
                }
                if let Some(b) = before {
                    if doc.timestamp > b {
                        return false;
                    }
                }
                true
            })
            .collect()
    }

    pub fn search(&self, query: &str) -> Vec<(f64, &Document)> {
        self.search_bm25(query, 1.5, 0.75)
    }

    pub fn stats(&self) -> IndexStats {
        IndexStats {
            num_documents: self.total_docs,
            num_terms: self.index.len(),
            avg_doc_length: self.avg_doc_length,
        }
    }

    pub fn extract_snippet(&self, doc: &Document, query: &str, context_words: usize) -> String {
        let query_terms = tokenize(query);
        let words: Vec<&str> = doc.content.split_whitespace().collect();

        if words.is_empty() {
            return String::new();
        }

        // Find first matching position
        let mut best_pos = 0;
        let mut best_matches = 0;

        for (i, window) in words.windows(context_words * 2).enumerate() {
            let window_text = window.join(" ").to_lowercase();
            let matches = query_terms
                .iter()
                .filter(|term| window_text.contains(term.as_str()))
                .count();
            if matches > best_matches {
                best_matches = matches;
                best_pos = i;
            }
        }

        // Extract snippet
        let start = best_pos.saturating_sub(context_words / 2);
        let end = (start + context_words * 2).min(words.len());
        let snippet_words = &words[start..end];

        let mut snippet = snippet_words.join(" ");

        // Highlight matching terms
        for term in &query_terms {
            let pattern = regex::escape(term);
            if let Ok(re) = regex::RegexBuilder::new(&pattern)
                .case_insensitive(true)
                .build()
            {
                snippet = re
                    .replace_all(&snippet, |caps: &regex::Captures| {
                        format!("**{}**", &caps[0])
                    })
                    .to_string();
            }
        }

        let prefix = if start > 0 { "..." } else { "" };
        let suffix = if end < words.len() { "..." } else { "" };

        format!("{}{}{}", prefix, snippet, suffix)
    }

    pub fn build_trie(&self) -> crate::trie::PrefixTrie {
        use crate::trie::PrefixTrie;

        let mut trie = PrefixTrie::new();

        for (term, postings) in &self.index {
            let frequency = postings.len();
            trie.insert(term, frequency);
        }

        trie
    }

    fn extract_domain(url: &str) -> String {
        if let Ok(parsed) = url::Url::parse(url) {
            if let Some(host) = parsed.host_str() {
                return host.to_string();
            }
        }
        String::new()
    }

    fn detect_content_type(url: &str, title: &str) -> String {
        let url_lower = url.to_lowercase();
        let title_lower = title.to_lowercase();

        if url_lower.ends_with('/') || url_lower.matches('/').count() <= 3 {
            "homepage".to_string()
        } else if url_lower.contains("/about") || title_lower.contains("about") {
            "about".to_string()
        } else if url_lower.contains("/contact") || title_lower.contains("contact") {
            "contact".to_string()
        } else {
            "article".to_string()
        }
    }
}

pub struct IndexStats {
    pub num_documents: usize,
    pub num_terms: usize,
    pub avg_doc_length: f64,
}

pub fn tokenize(text: &str) -> Vec<String> {
    let stemmer = Stemmer::create(Algorithm::English);

    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| s.len() >= 3)
        .filter(|s| !STOP_WORDS.contains(s))
        .map(|s| stemmer.stem(s).to_string())
        .collect()
}

pub fn levenshtein_distance(a: &str, b: &str) -> usize {
    let a_len = a.len();
    let b_len = b.len();

    if a_len == 0 {
        return b_len;
    }
    if b_len == 0 {
        return a_len;
    }

    let mut prev_row: Vec<usize> = (0..=b_len).collect();
    let mut curr_row = vec![0; b_len + 1];

    for (i, a_char) in a.chars().enumerate() {
        curr_row[0] = i + 1;

        for (j, b_char) in b.chars().enumerate() {
            let cost = if a_char == b_char { 0 } else { 1 };
            curr_row[j + 1] = (curr_row[j] + 1)
                .min(prev_row[j + 1] + 1)
                .min(prev_row[j] + cost);
        }

        std::mem::swap(&mut prev_row, &mut curr_row);
    }

    prev_row[b_len]
}
