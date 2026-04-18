use std::collections::HashMap;
use sha2::{Sha256, Digest};

pub struct EmbeddingCache {
    cache: HashMap<String, Vec<f32>>,
    max_size: usize,
    access_order: Vec<String>,
}

impl EmbeddingCache {
    pub fn new(max_size: usize) -> Self {
        Self {
            cache: HashMap::new(),
            max_size,
            access_order: Vec::new(),
        }
    }
    
    fn hash_text(text: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(text.as_bytes());
        format!("{:x}", hasher.finalize())
    }
    
    pub fn get(&mut self, text: &str) -> Option<Vec<f32>> {
        let key = Self::hash_text(text);
        
        if let Some(embedding) = self.cache.get(&key) {
            // Update access order (LRU)
            if let Some(pos) = self.access_order.iter().position(|k| k == &key) {
                self.access_order.remove(pos);
            }
            self.access_order.push(key);
            
            return Some(embedding.clone());
        }
        
        None
    }
    
    pub fn put(&mut self, text: &str, embedding: Vec<f32>) {
        let key = Self::hash_text(text);
        
        // Evict LRU if at capacity
        if self.cache.len() >= self.max_size && !self.cache.contains_key(&key) {
            if let Some(lru_key) = self.access_order.first().cloned() {
                self.cache.remove(&lru_key);
                self.access_order.remove(0);
            }
        }
        
        self.cache.insert(key.clone(), embedding);
        
        // Update access order
        if let Some(pos) = self.access_order.iter().position(|k| k == &key) {
            self.access_order.remove(pos);
        }
        self.access_order.push(key);
    }
    
    pub fn len(&self) -> usize {
        self.cache.len()
    }
    
    pub fn clear(&mut self) {
        self.cache.clear();
        self.access_order.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_cache_basic() {
        let mut cache = EmbeddingCache::new(2);
        
        cache.put("hello", vec![1.0, 2.0]);
        assert_eq!(cache.get("hello"), Some(vec![1.0, 2.0]));
        assert_eq!(cache.get("world"), None);
    }
    
    #[test]
    fn test_cache_lru_eviction() {
        let mut cache = EmbeddingCache::new(2);
        
        cache.put("a", vec![1.0]);
        cache.put("b", vec![2.0]);
        cache.put("c", vec![3.0]); // Should evict "a"
        
        assert_eq!(cache.get("a"), None);
        assert_eq!(cache.get("b"), Some(vec![2.0]));
        assert_eq!(cache.get("c"), Some(vec![3.0]));
    }
}
