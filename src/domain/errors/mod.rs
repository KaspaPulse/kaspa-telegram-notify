use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Node Error: {0}")]
    NodeError(String),
    #[error("Node connection failed: {0}")]
    NodeConnection(String),

    #[error("Database execution failed: {0}")]
    DatabaseError(String),

    #[error("API Request failed: {0}")]
    #[allow(dead_code)]
    ApiError(String),

    #[error("Internal processing error: {0}")]
    Internal(String),

    #[error("Entity not found: {0}")]
    #[allow(dead_code)]
    NotFound(String),
}

impl From<String> for AppError {
    fn from(err: String) -> Self {
        AppError::Internal(err)
    }
}

impl From<&str> for AppError {
    fn from(err: &str) -> Self {
        AppError::Internal(err.to_string())
    }
}
