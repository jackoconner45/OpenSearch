use anyhow::Result;
use rusqlite::Connection;
use std::collections::{HashMap, HashSet};

pub struct LinkGraph {
    pub outgoing: HashMap<String, Vec<String>>, // URL -> outgoing links
    pub incoming: HashMap<String, Vec<String>>, // URL -> incoming links
    pub all_urls: HashSet<String>,
}

impl LinkGraph {
    pub fn load_from_db(db_path: &str) -> Result<Self> {
        let conn = Connection::open(db_path)?;

        let mut outgoing: HashMap<String, Vec<String>> = HashMap::new();
        let mut incoming: HashMap<String, Vec<String>> = HashMap::new();
        let mut all_urls: HashSet<String> = HashSet::new();

        // Load all links
        let mut stmt = conn.prepare("SELECT from_url, to_url FROM links")?;
        let links = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;

        for link in links {
            let (from_url, to_url) = link?;

            all_urls.insert(from_url.clone());
            all_urls.insert(to_url.clone());

            outgoing
                .entry(from_url.clone())
                .or_insert_with(Vec::new)
                .push(to_url.clone());

            incoming
                .entry(to_url)
                .or_insert_with(Vec::new)
                .push(from_url);
        }

        Ok(Self {
            outgoing,
            incoming,
            all_urls,
        })
    }

    pub fn compute_pagerank(&self, damping: f64, iterations: usize) -> HashMap<String, f64> {
        let n = self.all_urls.len() as f64;
        let mut ranks: HashMap<String, f64> = self
            .all_urls
            .iter()
            .map(|url| (url.clone(), 1.0 / n))
            .collect();

        for _ in 0..iterations {
            let mut new_ranks: HashMap<String, f64> = HashMap::new();

            for url in &self.all_urls {
                let mut rank = (1.0 - damping) / n;

                // Add contributions from incoming links
                if let Some(incoming_links) = self.incoming.get(url) {
                    for from_url in incoming_links {
                        let default_rank = 1.0 / n;
                        let from_rank = ranks.get(from_url).unwrap_or(&default_rank);
                        let out_degree =
                            self.outgoing.get(from_url).map(|v| v.len()).unwrap_or(1) as f64;

                        rank += damping * (from_rank / out_degree);
                    }
                }

                new_ranks.insert(url.clone(), rank);
            }

            ranks = new_ranks;
        }

        // Normalize scores
        let sum: f64 = ranks.values().sum();
        if sum > 0.0 {
            for rank in ranks.values_mut() {
                *rank /= sum;
            }
        }

        ranks
    }
}
