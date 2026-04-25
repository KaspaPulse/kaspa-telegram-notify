#![allow(dead_code)]
use redis::aio::ConnectionManager;
use tracing::info;

#[allow(dead_code)]
pub struct RedisStateEngine {
    pub client: ConnectionManager,
}

impl RedisStateEngine {
    pub async fn init(redis_url: &str) -> Result<Self, redis::RedisError> {
        let client = redis::Client::open(redis_url)?;
        let manager = client.get_connection_manager().await?;
        info!("🌐 [REDIS] Distributed Memory Engine Connected!");
        Ok(Self { client: manager })
    }
}
