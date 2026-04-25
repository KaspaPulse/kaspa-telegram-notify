use regex::Regex;
use std::sync::OnceLock;
use tracing::warn;

pub struct AiFirewall;

impl AiFirewall {
    /// Advanced heuristic validation to prevent Prompt Injection & DoS
    pub fn validate_prompt(input: &str) -> bool {
        let lower = input.to_lowercase();

        // 1. Length Restriction (Prevent Resource Exhaustion/DoS)
        if input.len() > 1500 {
            warn!("[AI SECURITY] Rejected: Prompt exceeds maximum safe length.");
            return false;
        }

        // 2. Heuristic Pattern Matching (Defeats simple obfuscation)
        static INJECTION_REGEX: OnceLock<Regex> = OnceLock::new();
        let regex = INJECTION_REGEX.get_or_init(|| {
            Regex::new(r"(?i)(ignore.*previous|system.*prompt|bypass|jailbreak|forget.*all|disregard|developer.*mode|roleplay|instruction.*override|do.*anything.*now)").unwrap()
        });

        if regex.is_match(&lower) {
            warn!("[AI SECURITY] Rejected: Heuristic pattern match for prompt injection.");
            return false;
        }

        // 3. Tokenizer Attack Prevention (Excessive Special Characters)
        let special_chars = input
            .chars()
            .filter(|c| !c.is_alphanumeric() && !c.is_whitespace())
            .count();
        if !input.is_empty() && (special_chars as f64 / input.len() as f64) > 0.25 {
            warn!("[AI SECURITY] Rejected: Abnormal character distribution (Tokenizer Attack).");
            return false;
        }

        true
    }

    /// Enterprise-grade sanitization: Neutralize XML/HTML elements safely
    pub fn sanitize_prompt(input: &str) -> String {
        input
            .replace("<", "&lt;")
            .replace(">", "&gt;")
            .chars()
            .take(1500)
            .collect()
    }
}
