#![allow(dead_code)]
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use std::sync::OnceLock;
use tokio::fs;
use tokio::sync::Mutex;
use tracing::{error, info};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Document {
    pub title: String,
    pub content: String,
    #[serde(skip)]
    pub embedding: Option<Vec<f32>>,
}

pub struct CloudEmbedder {
    pub client: Client,
    pub api_key: String,
}

impl CloudEmbedder {
    pub fn new() -> anyhow::Result<Self> {
        info!("[RAG] Initializing Google Text-Embedding-004...");
        let api_key = std::env::var("GEMINI_API_KEY").unwrap_or_default();
        Ok(Self {
            client: Client::new(),
            api_key,
        })
    }

    pub async fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/text-embedding-004:embedContent?key={}",
            self.api_key
        );
        let body = json!({
            "model": "models/text-embedding-004",
            "content": { "parts": [{ "text": text }] }
        });

        let res = self.client.post(&url).json(&body).send().await?;
        let json_res: serde_json::Value = res.json().await?;

        if let Some(values) = json_res["embedding"]["values"].as_array() {
            let vec: Vec<f32> = values
                .iter()
                .filter_map(|v| v.as_f64().map(|f| f as f32))
                .collect();
            Ok(vec)
        } else {
            Err(anyhow::anyhow!("Failed to extract embeddings"))
        }
    }
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot_product / (norm_a * norm_b)
    }
}

static KNOWLEDGE_BASE: OnceLock<Vec<Document>> = OnceLock::new();
static EMBEDDER: OnceLock<Arc<Mutex<CloudEmbedder>>> = OnceLock::new();

pub async fn init_knowledge_base() {
    if let Ok(embedder) = CloudEmbedder::new() {
        let _ = EMBEDDER.set(Arc::new(Mutex::new(embedder)));
    } else {
        error!("[RAG] Failed to load Cloud Embedder.");
        return;
    }

    let file_content = fs::read_to_string("knowledge.json")
        .await
        .unwrap_or_else(|_| "[]".to_string());
    let mut docs: Vec<Document> = serde_json::from_str(&file_content).unwrap_or_default();

    let embedder_arc = EMBEDDER.get().unwrap().clone();

    info!("[RAG] Generating vector embeddings for knowledge base...");
    let embedder = embedder_arc.lock().await;
    for doc in docs.iter_mut() {
        if let Ok(emb) = embedder.embed(&doc.content).await {
            doc.embedding = Some(emb);
        }
    }

    let _ = KNOWLEDGE_BASE.set(docs);
    info!("[RAG] Knowledge base initialized securely via Cloud.");
}

pub async fn search_kaspa_docs(query: &str) -> String {
    let embedder_arc = match EMBEDDER.get() {
        Some(e) => e.clone(),
        None => return String::new(),
    };

    let query_embedding = {
        let embedder = embedder_arc.lock().await;
        match embedder.embed(query).await {
            Ok(emb) => emb,
            Err(_) => return String::new(),
        }
    };

    let docs = match KNOWLEDGE_BASE.get() {
        Some(d) => d,
        None => return String::new(),
    };

    let mut scored_docs: Vec<(&Document, f32)> = docs
        .iter()
        .filter_map(|d| {
            d.embedding
                .as_ref()
                .map(|emb| (d, cosine_similarity(&query_embedding, emb)))
        })
        .collect();

    scored_docs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    if scored_docs.is_empty() || scored_docs[0].1 < 0.2 {
        return "No highly relevant context found in local knowledge base.".to_string();
    }

    let top_docs: Vec<String> = scored_docs
        .into_iter()
        .take(2)
        .map(|(d, _)| format!("Title: {}\nContent: {}", d.title, d.content))
        .collect();
    top_docs.join("\n\n")
}
