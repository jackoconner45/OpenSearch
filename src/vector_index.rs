use anyhow::{bail, Result};
use rand::{rngs::StdRng, Rng, SeedableRng};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Write};

const DEFAULT_NUM_PLANES: usize = 14;
const MIN_EXACT_SCAN: usize = 256;
const MIN_CANDIDATE_MULTIPLIER: usize = 8;
const MAX_PROBE_HAMMING_DISTANCE: usize = 2;

#[derive(Debug, Serialize, Deserialize)]
pub struct VectorIndex {
    pub embeddings: Vec<(u32, Vec<f32>)>,
    pub dimension: usize,
    #[serde(skip)]
    ann_index: Option<RandomProjectionAnnIndex>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnIndexSummary {
    pub num_vectors: usize,
    pub dimension: usize,
    pub num_planes: usize,
    pub num_buckets: usize,
    pub max_bucket_size: usize,
    pub avg_bucket_size: f64,
}

#[derive(Debug, Clone)]
struct RandomProjectionAnnIndex {
    num_planes: usize,
    hyperplanes: Vec<Vec<f32>>,
    buckets: HashMap<u64, Vec<usize>>,
}

impl VectorIndex {
    pub fn new(dimension: usize) -> Self {
        Self {
            embeddings: Vec::new(),
            dimension,
            ann_index: None,
        }
    }

    pub fn from_embeddings(embeddings: Vec<(u32, Vec<f32>)>) -> Result<Self> {
        let dimension = embeddings.first().map(|(_, v)| v.len()).unwrap_or(0);
        let mut index = Self {
            embeddings,
            dimension,
            ann_index: None,
        };
        index.validate_dimensions()?;
        index.build_ann()?;
        Ok(index)
    }

    pub fn load(path: &str) -> Result<Self> {
        let mut file = File::open(path)?;
        let mut embeddings = Vec::new();
        let mut dimension = 0;

        loop {
            let mut doc_id_bytes = [0u8; 4];
            if file.read_exact(&mut doc_id_bytes).is_err() {
                break;
            }
            let doc_id = u32::from_le_bytes(doc_id_bytes);

            let mut len_bytes = [0u8; 4];
            file.read_exact(&mut len_bytes)?;
            let len = u32::from_le_bytes(len_bytes) as usize;

            if dimension == 0 {
                dimension = len;
            } else if len != dimension {
                bail!(
                    "inconsistent embedding dimension: expected {}, got {}",
                    dimension,
                    len
                );
            }

            let mut embedding = vec![0f32; len];
            for value in &mut embedding {
                let mut float_bytes = [0u8; 4];
                file.read_exact(&mut float_bytes)?;
                *value = f32::from_le_bytes(float_bytes);
            }

            embeddings.push((doc_id, embedding));
        }

        let mut index = Self {
            embeddings,
            dimension,
            ann_index: None,
        };
        index.validate_dimensions()?;
        index.build_ann()?;
        Ok(index)
    }

    pub fn search(&self, query_embedding: &[f32], k: usize) -> Vec<(f32, u32)> {
        if self.embeddings.len() <= MIN_EXACT_SCAN || self.ann_index.is_none() {
            return self.search_exact(query_embedding, k);
        }

        let candidates = self.ann_candidates(query_embedding, k);
        if candidates.len() < k {
            return self.search_exact(query_embedding, k);
        }

        self.rank_candidates(query_embedding, &candidates, k)
    }

    pub fn search_exact(&self, query_embedding: &[f32], k: usize) -> Vec<(f32, u32)> {
        let mut scores: Vec<(f32, u32)> = self
            .embeddings
            .iter()
            .map(|(doc_id, embedding)| (cosine_similarity(query_embedding, embedding), *doc_id))
            .collect();

        scores.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
        scores.into_iter().take(k).collect()
    }

    pub fn build_ann(&mut self) -> Result<()> {
        self.validate_dimensions()?;

        if self.dimension == 0 || self.embeddings.is_empty() {
            self.ann_index = None;
            return Ok(());
        }

        let ann =
            RandomProjectionAnnIndex::build(&self.embeddings, self.dimension, DEFAULT_NUM_PLANES);
        self.ann_index = Some(ann);
        Ok(())
    }

    pub fn build_hnsw(&mut self) -> Result<()> {
        self.build_ann()
    }

    pub fn save_hnsw(&self, path: &str) -> Result<()> {
        let summary = self.ann_summary();
        let mut file = File::create(path)?;
        file.write_all(serde_json::to_string_pretty(&summary)?.as_bytes())?;
        Ok(())
    }

    pub fn ann_summary(&self) -> AnnIndexSummary {
        match &self.ann_index {
            Some(index) => {
                let bucket_sizes: Vec<usize> = index.buckets.values().map(Vec::len).collect();
                let max_bucket_size = bucket_sizes.iter().copied().max().unwrap_or(0);
                let avg_bucket_size = if bucket_sizes.is_empty() {
                    0.0
                } else {
                    bucket_sizes.iter().sum::<usize>() as f64 / bucket_sizes.len() as f64
                };

                AnnIndexSummary {
                    num_vectors: self.embeddings.len(),
                    dimension: self.dimension,
                    num_planes: index.num_planes,
                    num_buckets: index.buckets.len(),
                    max_bucket_size,
                    avg_bucket_size,
                }
            }
            None => AnnIndexSummary {
                num_vectors: self.embeddings.len(),
                dimension: self.dimension,
                num_planes: 0,
                num_buckets: 0,
                max_bucket_size: 0,
                avg_bucket_size: 0.0,
            },
        }
    }

    fn validate_dimensions(&self) -> Result<()> {
        if self.dimension == 0 && !self.embeddings.is_empty() {
            bail!("vector index has embeddings but zero dimension");
        }

        for (doc_id, embedding) in &self.embeddings {
            if embedding.len() != self.dimension {
                bail!(
                    "embedding dimension mismatch for doc {}: expected {}, got {}",
                    doc_id,
                    self.dimension,
                    embedding.len()
                );
            }
        }

        Ok(())
    }

    fn ann_candidates(&self, query_embedding: &[f32], k: usize) -> Vec<usize> {
        let Some(ann_index) = &self.ann_index else {
            return (0..self.embeddings.len()).collect();
        };

        let target_candidates = (k * MIN_CANDIDATE_MULTIPLIER).max(k);
        ann_index.candidates(query_embedding, target_candidates)
    }

    fn rank_candidates(
        &self,
        query_embedding: &[f32],
        candidates: &[usize],
        k: usize,
    ) -> Vec<(f32, u32)> {
        let mut scores: Vec<(f32, u32)> = candidates
            .iter()
            .filter_map(|idx| self.embeddings.get(*idx))
            .map(|(doc_id, embedding)| (cosine_similarity(query_embedding, embedding), *doc_id))
            .collect();

        scores.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
        scores.truncate(k);
        scores
    }
}

impl RandomProjectionAnnIndex {
    fn build(embeddings: &[(u32, Vec<f32>)], dimension: usize, num_planes: usize) -> Self {
        let mut rng = StdRng::seed_from_u64(5_134_245);
        let mut hyperplanes = Vec::with_capacity(num_planes);

        for _ in 0..num_planes {
            let mut plane = Vec::with_capacity(dimension);
            for _ in 0..dimension {
                plane.push(rng.gen_range(-1.0..1.0));
            }
            normalize_vector(&mut plane);
            hyperplanes.push(plane);
        }

        let mut buckets: HashMap<u64, Vec<usize>> = HashMap::new();
        for (idx, (_, embedding)) in embeddings.iter().enumerate() {
            let signature = compute_signature(&hyperplanes, embedding);
            buckets.entry(signature).or_default().push(idx);
        }

        Self {
            num_planes,
            hyperplanes,
            buckets,
        }
    }

    fn candidates(&self, query_embedding: &[f32], target: usize) -> Vec<usize> {
        if self.buckets.is_empty() {
            return Vec::new();
        }

        let signature = compute_signature(&self.hyperplanes, query_embedding);
        let mut seen = HashSet::new();
        let mut candidates = Vec::new();

        self.extend_bucket(signature, &mut seen, &mut candidates);
        if candidates.len() >= target {
            return candidates;
        }

        for distance in 1..=MAX_PROBE_HAMMING_DISTANCE {
            for neighbor in signatures_within_hamming_distance(signature, self.num_planes, distance)
            {
                self.extend_bucket(neighbor, &mut seen, &mut candidates);
                if candidates.len() >= target {
                    return candidates;
                }
            }
        }

        let mut remaining_buckets: Vec<(u32, &Vec<usize>)> = self
            .buckets
            .iter()
            .map(|(bucket_signature, bucket)| ((bucket_signature ^ signature).count_ones(), bucket))
            .collect();
        remaining_buckets.sort_by_key(|(distance, _)| *distance);

        for (_, bucket) in remaining_buckets {
            for idx in bucket {
                if seen.insert(*idx) {
                    candidates.push(*idx);
                }
            }
            if candidates.len() >= target {
                break;
            }
        }

        candidates
    }

    fn extend_bucket(
        &self,
        signature: u64,
        seen: &mut HashSet<usize>,
        candidates: &mut Vec<usize>,
    ) {
        if let Some(bucket) = self.buckets.get(&signature) {
            for idx in bucket {
                if seen.insert(*idx) {
                    candidates.push(*idx);
                }
            }
        }
    }
}

fn compute_signature(hyperplanes: &[Vec<f32>], embedding: &[f32]) -> u64 {
    hyperplanes
        .iter()
        .enumerate()
        .fold(0u64, |signature, (bit, plane)| {
            let dot: f32 = plane.iter().zip(embedding.iter()).map(|(a, b)| a * b).sum();
            if dot >= 0.0 {
                signature | (1u64 << bit)
            } else {
                signature
            }
        })
}

fn normalize_vector(values: &mut [f32]) {
    let norm = values.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm > 0.0 {
        for value in values {
            *value /= norm;
        }
    }
}

fn signatures_within_hamming_distance(
    signature: u64,
    num_planes: usize,
    distance: usize,
) -> Vec<u64> {
    let mut neighbors = Vec::new();
    let mut selection = Vec::with_capacity(distance);
    generate_bit_flips(
        signature,
        num_planes,
        distance,
        0,
        &mut selection,
        &mut neighbors,
    );
    neighbors
}

fn generate_bit_flips(
    signature: u64,
    num_planes: usize,
    remaining: usize,
    start: usize,
    selection: &mut Vec<usize>,
    output: &mut Vec<u64>,
) {
    if remaining == 0 {
        let mut flipped = signature;
        for bit in selection.iter() {
            flipped ^= 1u64 << bit;
        }
        output.push(flipped);
        return;
    }

    for bit in start..num_planes {
        selection.push(bit);
        generate_bit_flips(
            signature,
            num_planes,
            remaining - 1,
            bit + 1,
            selection,
            output,
        );
        selection.pop();
    }
}

pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot / (norm_a * norm_b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_clustered_index() -> VectorIndex {
        let mut embeddings = Vec::new();
        for i in 0..300u32 {
            embeddings.push((i, vec![1.0, i as f32 * 0.0001, 0.0, 0.0]));
        }
        for i in 300..600u32 {
            embeddings.push((i, vec![0.0, 1.0, (i - 300) as f32 * 0.0001, 0.0]));
        }
        VectorIndex::from_embeddings(embeddings).unwrap()
    }

    #[test]
    fn exact_and_ann_return_same_top_match_for_cluster_query() {
        let index = make_clustered_index();
        let query = vec![1.0, 0.01, 0.0, 0.0];

        let exact = index.search_exact(&query, 5);
        let approx = index.search(&query, 5);

        assert_eq!(
            exact.first().map(|(_, doc_id)| *doc_id),
            approx.first().map(|(_, doc_id)| *doc_id)
        );
    }

    #[test]
    fn ann_builds_summary() {
        let index = make_clustered_index();
        let summary = index.ann_summary();

        assert_eq!(summary.num_vectors, 600);
        assert!(summary.num_buckets > 1);
        assert!(summary.num_planes > 0);
    }

    #[test]
    fn exact_search_handles_zero_vectors() {
        let index =
            VectorIndex::from_embeddings(vec![(1, vec![0.0, 0.0]), (2, vec![1.0, 0.0])]).unwrap();
        let results = index.search_exact(&[0.0, 0.0], 2);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, 0.0);
    }
}
