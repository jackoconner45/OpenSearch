use crate::indexer::{InvertedIndex, Document};
use anyhow::Result;
use serde::{Serialize, Deserialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

#[derive(Debug, Serialize, Deserialize)]
pub struct ShardedIndex {
    shards: Vec<InvertedIndex>,
    num_shards: usize,
}

impl ShardedIndex {
    pub fn new(num_shards: usize) -> Self {
        let mut shards = Vec::with_capacity(num_shards);
        for _ in 0..num_shards {
            shards.push(InvertedIndex::new());
        }
        Self { shards, num_shards }
    }
    
    fn route_to_shard(&self, url: &str) -> usize {
        let mut hasher = DefaultHasher::new();
        url.hash(&mut hasher);
        (hasher.finish() % self.num_shards as u64) as usize
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
        let shard_id = self.route_to_shard(&url);
        self.shards[shard_id].add_document(url, title, content, headings, word_count, pagerank);
    }
    
    pub fn search(&self, query: &str) -> Vec<(f64, &Document)> {
        let mut all_results: Vec<(f64, &Document)> = Vec::new();
        
        // Search all shards
        for shard in &self.shards {
            let shard_results = shard.search(query);
            all_results.extend(shard_results);
        }
        
        // Sort by score
        all_results.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
        
        all_results.into_iter().take(20).collect()
    }
    
    pub fn filter_results<'a>(
        &'a self,
        results: Vec<(f64, &'a Document)>,
        domain: Option<&str>,
        after: Option<u64>,
        before: Option<u64>,
    ) -> Vec<(f64, &'a Document)> {
        results.into_iter()
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
    
    pub fn stats(&self) -> ShardedStats {
        let mut total_docs = 0;
        let mut total_terms = 0;
        
        for shard in &self.shards {
            let shard_stats = shard.stats();
            total_docs += shard_stats.num_documents;
            total_terms += shard_stats.num_terms;
        }
        
        ShardedStats {
            num_shards: self.num_shards,
            total_documents: total_docs,
            total_terms: total_terms,
        }
    }
    
    pub fn finalize(&mut self) {
        for shard in &mut self.shards {
            shard.finalize();
        }
    }
    
    pub fn save(&self, base_path: &str) -> Result<()> {
        for (i, shard) in self.shards.iter().enumerate() {
            let shard_path = format!("{}.shard{}", base_path, i);
            shard.save(&shard_path)?;
        }
        
        // Save metadata
        let meta = ShardMetadata {
            num_shards: self.num_shards,
        };
        let meta_path = format!("{}.meta", base_path);
        std::fs::write(meta_path, serde_json::to_vec(&meta)?)?;
        
        Ok(())
    }
    
    pub fn load(base_path: &str) -> Result<Self> {
        // Load metadata
        let meta_path = format!("{}.meta", base_path);
        let meta_data = std::fs::read(meta_path)?;
        let meta: ShardMetadata = serde_json::from_slice(&meta_data)?;
        
        let mut shards = Vec::with_capacity(meta.num_shards);
        for i in 0..meta.num_shards {
            let shard_path = format!("{}.shard{}", base_path, i);
            shards.push(InvertedIndex::load(&shard_path)?);
        }
        
        Ok(Self {
            shards,
            num_shards: meta.num_shards,
        })
    }
}

#[derive(Serialize, Deserialize)]
struct ShardMetadata {
    num_shards: usize,
}

pub struct ShardedStats {
    pub num_shards: usize,
    pub total_documents: usize,
    pub total_terms: usize,
}
