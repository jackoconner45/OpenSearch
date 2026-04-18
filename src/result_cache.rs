use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use serde::{Serialize, Deserialize};

const TTL_SECONDS: u64 = 7 * 24 * 60 * 60; // 7 days

#[derive(Clone, Serialize, Deserialize)]
pub struct CachedResult {
    pub doc_ids: Vec<u32>,
    pub scores: Vec<f64>,
    pub timestamp: u64,
}

pub struct ResultCache {
    cache: HashMap<String, CachedResult>,
    access_order: Vec<String>,
    max_size: usize,
}

impl ResultCache {
    pub fn new(max_size: usize) -> Self {
        Self {
            cache: HashMap::new(),
            access_order: Vec::new(),
            max_size,
        }
    }
    
    pub(crate) fn current_timestamp() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }
    
    fn is_expired(&self, cached: &CachedResult) -> bool {
        let now = Self::current_timestamp();
        now - cached.timestamp > TTL_SECONDS
    }
    
    pub fn get(&mut self, query: &str) -> Option<CachedResult> {
        let key = query.to_lowercase();
        
        if let Some(cached) = self.cache.get(&key) {
            // Check if expired
            if self.is_expired(cached) {
                self.cache.remove(&key);
                if let Some(pos) = self.access_order.iter().position(|k| k == &key) {
                    self.access_order.remove(pos);
                }
                return None;
            }
            
            // Update access order (LRU)
            if let Some(pos) = self.access_order.iter().position(|k| k == &key) {
                self.access_order.remove(pos);
            }
            self.access_order.push(key);
            
            return Some(cached.clone());
        }
        
        None
    }
    
    pub fn put(&mut self, query: &str, doc_ids: Vec<u32>, scores: Vec<f64>) {
        let key = query.to_lowercase();
        
        // Evict LRU if at capacity
        if self.cache.len() >= self.max_size && !self.cache.contains_key(&key) {
            if let Some(lru_key) = self.access_order.first().cloned() {
                self.cache.remove(&lru_key);
                self.access_order.remove(0);
            }
        }
        
        let cached = CachedResult {
            doc_ids,
            scores,
            timestamp: Self::current_timestamp(),
        };
        
        self.cache.insert(key.clone(), cached);
        
        // Update access order
        if let Some(pos) = self.access_order.iter().position(|k| k == &key) {
            self.access_order.remove(pos);
        }
        self.access_order.push(key);
    }
    
    pub fn cleanup(&mut self) {
        let now = Self::current_timestamp();
        let expired_keys: Vec<String> = self.cache
            .iter()
            .filter(|(_, cached)| now - cached.timestamp > TTL_SECONDS)
            .map(|(k, _)| k.clone())
            .collect();
        
        for key in expired_keys {
            self.cache.remove(&key);
            if let Some(pos) = self.access_order.iter().position(|k| k == &key) {
                self.access_order.remove(pos);
            }
        }
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
        let mut cache = ResultCache::new(10);
        
        cache.put("test query", vec![1, 2, 3], vec![1.0, 0.9, 0.8]);
        let result = cache.get("test query").unwrap();
        
        assert_eq!(result.doc_ids, vec![1, 2, 3]);
        assert_eq!(result.scores, vec![1.0, 0.9, 0.8]);
    }
    
    #[test]
    fn test_cache_case_insensitive() {
        let mut cache = ResultCache::new(10);
        
        cache.put("Test Query", vec![1], vec![1.0]);
        assert!(cache.get("test query").is_some());
        assert!(cache.get("TEST QUERY").is_some());
    }
    
    #[test]
    fn test_lru_eviction() {
        let mut cache = ResultCache::new(2);
        
        cache.put("a", vec![1], vec![1.0]);
        cache.put("b", vec![2], vec![2.0]);
        cache.put("c", vec![3], vec![3.0]); // Should evict "a"
        
        assert!(cache.get("a").is_none());
        assert!(cache.get("b").is_some());
        assert!(cache.get("c").is_some());
    }

    #[test]
    fn test_ttl_expiry() {
        let mut cache = ResultCache::new(10);
        // Insert an entry with a timestamp older than TTL
        let expired_ts = ResultCache::current_timestamp() - TTL_SECONDS - 1;
        cache.cache.insert("expired".to_string(), CachedResult {
            doc_ids: vec![1],
            scores: vec![1.0],
            timestamp: expired_ts,
        });
        cache.access_order.push("expired".to_string());

        assert!(cache.get("expired").is_none(), "expired entry should not be returned");
        assert_eq!(cache.len(), 0, "expired entry should be removed");
    }

    #[test]
    fn test_cleanup_removes_expired() {
        let mut cache = ResultCache::new(10);
        let expired_ts = ResultCache::current_timestamp() - TTL_SECONDS - 1;
        cache.cache.insert("old".to_string(), CachedResult {
            doc_ids: vec![1],
            scores: vec![1.0],
            timestamp: expired_ts,
        });
        cache.access_order.push("old".to_string());
        cache.put("fresh", vec![2], vec![2.0]);

        cache.cleanup();
        assert!(cache.get("old").is_none());
        assert!(cache.get("fresh").is_some());
    }
}
