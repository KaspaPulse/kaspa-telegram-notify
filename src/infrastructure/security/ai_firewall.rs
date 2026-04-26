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
            Regex::new(r"(?i)(ignore.{0,20}previous|system.{0,20}prompt|bypass|jailbreak|forget.{0,20}all|disregard|developer.{0,20}mode|roleplay|instruction.{0,20}override|do.{0,20}anything|new.{0,20}instructions|تجاهل.{0,20}التعليمات|انس.{0,20}التعليمات|انت.{0,20}الان|انسى.{0,20}كل)").unwrap()
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
