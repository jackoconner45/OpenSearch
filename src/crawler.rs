use anyhow::{Context, Result};
use rand::Rng;
use reqwest::{header, Client};
use scraper::{Html, Selector};
use sha2::{Digest, Sha256};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use url::Url;

const USER_AGENTS: &[&str] = &[
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/121.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/121.0.0.0 Safari/537.36",
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/121.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:122.0) Gecko/20100101 Firefox/122.0",
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 14.3; rv:122.0) Gecko/20100101 Firefox/122.0",
];

pub struct Crawler {
    client: Client,
    link_selector: Selector,
}

impl Crawler {
    pub fn new() -> Result<Self> {
        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::ACCEPT,
            "text/html,application/xhtml+xml,application/xml;q=0.9,image/webp,*/*;q=0.8".parse()?,
        );
        headers.insert(header::ACCEPT_LANGUAGE, "en-US,en;q=0.9".parse()?);
        headers.insert(header::ACCEPT_ENCODING, "gzip, deflate, br".parse()?);
        headers.insert(header::DNT, "1".parse()?);
        headers.insert(header::CONNECTION, "keep-alive".parse()?);
        headers.insert("Upgrade-Insecure-Requests", "1".parse()?);

        let client = Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::limited(5))
            .gzip(true)
            .brotli(true)
            .deflate(true)
            .pool_max_idle_per_host(10) // Speed: reuse connections
            .pool_idle_timeout(Duration::from_secs(90))
            .build()?;

        let link_selector = Selector::parse("a[href]").unwrap();

        Ok(Self {
            client,
            link_selector,
        })
    }

    fn random_user_agent(&self) -> &'static str {
        let mut rng = rand::thread_rng();
        USER_AGENTS[rng.gen_range(0..USER_AGENTS.len())]
    }

    pub async fn fetch(&self, url: &str) -> Result<CrawlResult> {
        let user_agent = self.random_user_agent();

        let response = self
            .client
            .get(url)
            .header(header::USER_AGENT, user_agent)
            .header(header::REFERER, "https://www.google.com/")
            .send()
            .await
            .context("Failed to fetch URL")?;

        let status = response.status().as_u16();
        let html = response.text().await?;

        let content_hash = compute_hash(&html);
        let links = self.extract_links(&html, url)?;

        Ok(CrawlResult {
            url: url.to_string(),
            html,
            status_code: status,
            content_hash,
            links,
            crawled_at: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
        })
    }

    fn extract_links(&self, html: &str, base_url: &str) -> Result<Vec<String>> {
        let document = Html::parse_document(html);
        let base = Url::parse(base_url)?;

        let mut links = Vec::new();
        for element in document.select(&self.link_selector) {
            if let Some(href) = element.value().attr("href") {
                if let Ok(absolute_url) = base.join(href) {
                    if absolute_url.scheme() == "http" || absolute_url.scheme() == "https" {
                        links.push(absolute_url.to_string());
                    }
                }
            }
        }

        Ok(links)
    }
}

/// Fast hash computation
pub fn compute_hash(content: &str) -> String {
    format!("{:x}", Sha256::digest(content.as_bytes()))
}

pub struct CrawlResult {
    pub url: String,
    pub html: String,
    pub status_code: u16,
    pub content_hash: String,
    pub links: Vec<String>,
    pub crawled_at: u64,
}
