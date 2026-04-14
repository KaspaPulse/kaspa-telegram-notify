#![allow(dead_code)]
use thiserror::Error;

#[derive(Error, Debug)]
pub enum EnterpriseError {
    #[error("Rate limit exceeded for user: {0}")]
    RateLimited(i64),

    #[error("Database constraint violation: {0}")]
    DatabaseError(#[from] sqlx::Error),

    #[error("Node RPC communication failed: {0}")]
    RpcError(String),

    #[error("AI Engine API error: {0}")]
    AiEngineError(String),
}
