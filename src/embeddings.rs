use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::time::{sleep, Duration};

const OLLAMA_API_URL: &str = "http://localhost:11434/api/embed";
const MODEL_NAME: &str = "nomic-embed-text";
const MAX_TEXT_LENGTH: usize = 2000;

// Rate limiters
static NVIDIA_SEMAPHORE: once_cell::sync::Lazy<Arc<Semaphore>> =
    once_cell::sync::Lazy::new(|| Arc::new(Semaphore::new(1)));
static CLOUDFLARE_SEMAPHORE: once_cell::sync::Lazy<Arc<Semaphore>> =
    once_cell::sync::Lazy::new(|| Arc::new(Semaphore::new(1)));

const NVIDIA_RPM: u64 = 30;
const CLOUDFLARE_RPM: u64 = 40;

#[derive(Serialize)]
struct OllamaRequest {
    model: String,
    input: Vec<String>,
}

#[derive(Deserialize)]
struct OllamaResponse {
    embeddings: Vec<Vec<f32>>,
}

#[derive(Serialize)]
struct NvidiaRequest {
    input: Vec<String>,
    model: String,
    input_type: String,
    encoding_format: String,
    truncate: String,
}

#[derive(Deserialize)]
struct NvidiaResponse {
    data: Vec<NvidiaEmbedding>,
}

#[derive(Deserialize)]
struct NvidiaEmbedding {
    embedding: Vec<f32>,
}

#[derive(Serialize)]
struct CloudflareRequest {
    text: Vec<String>,
}

#[derive(Deserialize)]
struct CloudflareResponse {
    result: CloudflareResult,
}

#[derive(Deserialize)]
struct CloudflareResult {
    data: Vec<Vec<f32>>,
}

pub async fn embed(text: &str) -> Result<Vec<f32>> {
    let batch = embed_batch(&[text]).await?;
    Ok(batch.into_iter().next().unwrap_or_default())
}

pub async fn embed_batch(texts: &[&str]) -> Result<Vec<Vec<f32>>> {
    let cleaned: Vec<String> = texts
        .iter()
        .map(|text| {
            let text = if text.len() > MAX_TEXT_LENGTH {
                &text[..MAX_TEXT_LENGTH]
            } else {
                text
            };
            text.trim().to_string()
        })
        .collect();

    if cleaned.iter().all(|t| t.is_empty()) {
        return Ok(vec![vec![0.0; 768]; texts.len()]);
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()?;

    let request = OllamaRequest {
        model: MODEL_NAME.to_string(),
        input: cleaned,
    };

    let response = client
        .post(OLLAMA_API_URL)
        .json(&request)
        .send()
        .await
        .context("Failed to send request to Ollama")?;

    let status = response.status();
    let response_text = response.text().await?;

    if !status.is_success() {
        anyhow::bail!("Ollama API error ({}): {}", status, response_text);
    }

    let embed_response: OllamaResponse = serde_json::from_str(&response_text)
        .context(format!("Failed to parse response: {}", response_text))?;

    Ok(embed_response.embeddings)
}

pub async fn nvidia_embed_batch(
    texts: &[&str],
    api_key: &str,
    model: &str,
) -> Result<Vec<Vec<f32>>> {
    // Rate limiting
    let _permit = NVIDIA_SEMAPHORE.acquire().await?;

    let cleaned: Vec<String> = texts
        .iter()
        .map(|text| {
            let text = if text.len() > MAX_TEXT_LENGTH {
                &text[..MAX_TEXT_LENGTH]
            } else {
                text
            };
            text.trim().to_string()
        })
        .collect();

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()?;

    let request = NvidiaRequest {
        input: cleaned,
        model: model.to_string(),
        input_type: "query".to_string(),
        encoding_format: "float".to_string(),
        truncate: "NONE".to_string(),
    };

    let response = client
        .post("https://integrate.api.nvidia.com/v1/embeddings")
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&request)
        .send()
        .await?;

    let status = response.status();
    let response_text = response.text().await?;

    if !status.is_success() {
        anyhow::bail!("NVIDIA API error ({}): {}", status, response_text);
    }

    let embed_response: NvidiaResponse = serde_json::from_str(&response_text)?;
    let embeddings = embed_response
        .data
        .into_iter()
        .map(|e| e.embedding)
        .collect();

    // Rate limit delay
    sleep(Duration::from_millis(60000 / NVIDIA_RPM)).await;

    Ok(embeddings)
}

pub async fn cloudflare_embed_batch(
    texts: &[&str],
    api_token: &str,
    account_id: &str,
    model: &str,
) -> Result<Vec<Vec<f32>>> {
    // Rate limiting
    let _permit = CLOUDFLARE_SEMAPHORE.acquire().await?;

    let cleaned: Vec<String> = texts
        .iter()
        .map(|text| {
            let text = if text.len() > MAX_TEXT_LENGTH {
                &text[..MAX_TEXT_LENGTH]
            } else {
                text
            };
            text.trim().to_string()
        })
        .collect();

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()?;

    let request = CloudflareRequest { text: cleaned };

    let url = format!(
        "https://api.cloudflare.com/client/v4/accounts/{}/ai/run/{}",
        account_id, model
    );

    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", api_token))
        .json(&request)
        .send()
        .await?;

    let status = response.status();
    let response_text = response.text().await?;

    if !status.is_success() {
        anyhow::bail!("Cloudflare API error ({}): {}", status, response_text);
    }

    let embed_response: CloudflareResponse = serde_json::from_str(&response_text)?;

    // Rate limit delay
    sleep(Duration::from_millis(60000 / CLOUDFLARE_RPM)).await;

    Ok(embed_response.result.data)
}
