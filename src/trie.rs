use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize)]
pub struct TrieNode {
    children: HashMap<char, TrieNode>,
    is_end: bool,
    frequency: usize,
}

impl TrieNode {
    fn new() -> Self {
        Self {
            children: HashMap::new(),
            is_end: false,
            frequency: 0,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PrefixTrie {
    root: TrieNode,
}

impl PrefixTrie {
    pub fn new() -> Self {
        Self {
            root: TrieNode::new(),
        }
    }

    pub fn insert(&mut self, word: &str, frequency: usize) {
        let mut node = &mut self.root;

        for ch in word.chars() {
            node = node.children.entry(ch).or_insert_with(TrieNode::new);
        }

        node.is_end = true;
        node.frequency = frequency;
    }

    pub fn suggest(&self, prefix: &str, limit: usize) -> Vec<(String, usize)> {
        // Find the node for the prefix
        let mut node = &self.root;

        for ch in prefix.chars() {
            match node.children.get(&ch) {
                Some(n) => node = n,
                None => return Vec::new(), // Prefix not found
            }
        }

        // Collect all words with this prefix
        let mut results = Vec::new();
        self.collect_words(node, prefix.to_string(), &mut results);

        // Sort by frequency (descending) and take top N
        results.sort_by(|a, b| b.1.cmp(&a.1));
        results.into_iter().take(limit).collect()
    }

    fn collect_words(&self, node: &TrieNode, current: String, results: &mut Vec<(String, usize)>) {
        if node.is_end {
            results.push((current.clone(), node.frequency));
        }

        for (ch, child) in &node.children {
            let mut next = current.clone();
            next.push(*ch);
            self.collect_words(child, next, results);
        }
    }
}
