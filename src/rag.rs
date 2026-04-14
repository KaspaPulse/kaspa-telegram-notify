#![allow(dead_code)]
use candle_core::{Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{BertModel, Config};
use hf_hub::{api::sync::Api, Repo, RepoType};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex, OnceLock};
use tokenizers::Tokenizer;
use tokio::fs;
use tracing::{error, info};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Document {
    pub title: String,
    pub content: String,
    #[serde(skip)]
    pub embedding: Option<Vec<f32>>,
}

pub struct LocalEmbedder {
    pub model: BertModel,
    pub tokenizer: Tokenizer,
    pub device: Device,
}

impl LocalEmbedder {
    pub fn new() -> anyhow::Result<Self> {
        info!("[RAG] Initializing Local MiniLM Embedding Model...");
        let device = Device::cuda_if_available(0).unwrap_or(Device::Cpu);

        let api = Api::new()?;
        let repo = api.repo(Repo::with_revision(
            "sentence-transformers/all-MiniLM-L6-v2".to_string(),
            RepoType::Model,
            "main".to_string(),
        ));

        let config_path = repo.get("config.json")?;
        let tokenizer_path = repo.get("tokenizer.json")?;
        let weights_path = repo.get("model.safetensors")?;

        let config: Config = serde_json::from_str(&std::fs::read_to_string(config_path)?)?;
        let tokenizer = Tokenizer::from_file(tokenizer_path).map_err(anyhow::Error::msg)?;

        let vb = unsafe {
            VarBuilder::from_mmaped_safetensors(&[weights_path], candle_core::DType::F32, &device)?
        };
        let model = BertModel::load(vb, &config)?;

        info!("[RAG] MiniLM Model loaded successfully.");
        Ok(Self {
            model,
            tokenizer,
            device,
        })
    }

    pub fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        let tokens = self
            .tokenizer
            .encode(text, true)
            .map_err(anyhow::Error::msg)?;
        let token_ids = tokens.get_ids();

        let token_ids_tensor = Tensor::new(token_ids, &self.device)?.unsqueeze(0)?;
        let token_type_ids = Tensor::zeros_like(&token_ids_tensor)?;

        // Pass None for the attention_mask in standard forward passes
        let embeddings = self
            .model
            .forward(&token_ids_tensor, &token_type_ids, None)?;

        // Apply Mean Pooling to the embeddings
        let sum_embeddings = embeddings.sum(1)?;
        let seq_len = token_ids.len() as f64;
        let mean_embeddings = (sum_embeddings / seq_len)?;

        let vec: Vec<f32> = mean_embeddings.squeeze(0)?.to_vec1()?;
        Ok(vec)
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
static EMBEDDER: OnceLock<Arc<Mutex<LocalEmbedder>>> = OnceLock::new();

pub async fn init_knowledge_base() {
    if let Ok(embedder) = LocalEmbedder::new() {
        let _ = EMBEDDER.set(Arc::new(Mutex::new(embedder)));
    } else {
        error!("[RAG] Failed to load local embedder model.");
        return;
    }

    let file_content = fs::read_to_string("knowledge.json")
        .await
        .unwrap_or_else(|_| "[]".to_string());
    let mut docs: Vec<Document> = serde_json::from_str(&file_content).unwrap_or_default();

    let embedder_arc = EMBEDDER.get().unwrap().clone();

    info!("[RAG] Generating vector embeddings for local knowledge base...");
    for doc in docs.iter_mut() {
        let embedder = embedder_arc.lock().unwrap();
        if let Ok(emb) = embedder.embed(&doc.content) {
            doc.embedding = Some(emb);
        }
    }

    let _ = KNOWLEDGE_BASE.set(docs);
    info!("[RAG] Knowledge base initialized with local embeddings.");
}

pub fn search_kaspa_docs(query: &str) -> String {
    let embedder_arc = match EMBEDDER.get() {
        Some(e) => e.clone(),
        None => return String::new(),
    };

    let query_embedding = {
        let embedder = embedder_arc.lock().unwrap();
        match embedder.embed(query) {
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

    // Sort descending by relevance score
    scored_docs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    // Threshold for relevance (Ignore irrelevant search results)
    if scored_docs.is_empty() || scored_docs[0].1 < 0.2 {
        return "No highly relevant context found in local knowledge base.".to_string();
    }

    // Grab the top 2 relevant documents
    let top_docs: Vec<String> = scored_docs
        .into_iter()
        .take(2)
        .map(|(d, _)| format!("Title: {}\nContent: {}", d.title, d.content))
        .collect();

    top_docs.join("\n\n")
}
