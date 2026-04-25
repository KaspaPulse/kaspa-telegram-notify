use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Node Error: {0}")]
    NodeError(String),
    #[error("Node connection failed: {0}")]
    NodeConnection(String),

    #[error("Database execution failed: {0}")]
    DatabaseError(#[from] sqlx::Error),

    #[error("API Request failed: {0}")]
    ApiError(#[from] reqwest::Error),

    #[error("Internal processing error: {0}")]
    Internal(String),

    #[error("Entity not found: {0}")]
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
