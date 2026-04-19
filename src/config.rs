use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct AppConfig {
    pub ai_system_prompt: String,
    pub rss_feeds: Vec<String>,
    pub donation_wallet: String,
}

impl AppConfig {
    pub fn load_defaults() -> Self {
        Self {
            ai_system_prompt: std::env::var("AI_SYSTEM_PROMPT").unwrap_or_else(|_| {
                "You are the 'Kaspa Sovereign Intelligence' (V2.0-Hardened).\n\n[STRICT ARCHITECTURAL PROTOCOLS]\n1. TRUTH OBLIGATION: Kaspa does NOT support Smart Contracts V2 yet. If asked, explicitly state that current development is focused on 10 BPS and KIPs.\n2. NO HALLUCINATION: Do not invent RPC methods or libraries.\n3. PROFESSIONAL FORMATTING: Use Clean Telegram HTML only.\n4. LANGUAGE: Explain in Arabic, keep technical code in English (ASCII).".to_string()
            }),
            rss_feeds: vec![
                "https://medium.com/feed/kaspa-currency".to_string(),
                "https://github.com/kaspanet/rusty-kaspa/releases.atom".to_string(),
                "https://kaspa.org/feed/".to_string(),
            ],
            donation_wallet: std::env::var("DONATION_WALLET").unwrap_or_else(|_| {
                "kaspa:qz0yqq8z3twwgg7lq2mjzg6w4edqys45w2wslz7tym2tc6s84580vvx9zr44g".to_string()
            }),
        }
    }
}
