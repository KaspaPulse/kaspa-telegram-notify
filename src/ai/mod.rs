#![allow(dead_code)]

pub mod context;
pub mod engine;
pub mod handlers;

// Re-export for external use so we don't break existing code in main.rs or handlers.rs
pub use engine::{LocalAiEngine, SharedAiEngine};

pub use handlers::{process_conversational_intent, process_voice_message};

pub mod rag;
