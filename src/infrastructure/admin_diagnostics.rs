use chrono::{DateTime, Utc};
use sqlx::PgPool;

#[derive(Debug)]
pub struct AdminDatabaseSnapshot {
    pub connection: Result<(), String>,
    pub users_count: Result<i64, String>,
    pub wallets_count: Result<i64, String>,
    pub mined_count: Result<i64, String>,
    pub settings_count: Result<i64, String>,
    pub last_alert: Result<Option<DateTime<Utc>>, String>,
}

impl AdminDatabaseSnapshot {
    pub fn connection_ok(&self) -> bool {
        self.connection.is_ok()
    }

    pub fn required_queries_ok(&self) -> bool {
        self.users_count.is_ok()
            && self.wallets_count.is_ok()
            && self.mined_count.is_ok()
            && self.settings_count.is_ok()
            && self.last_alert.is_ok()
    }

    pub fn database_status(&self) -> &'static str {
        if !self.connection_ok() {
            "FAILED"
        } else if !self.required_queries_ok() {
            "DEGRADED"
        } else {
            "OK"
        }
    }
}

fn query_error(operation: &'static str, error: sqlx::Error) -> String {
    tracing::error!(
        operation,
        error = %crate::utils::sanitize_for_log(&error.to_string()),
        "[DATABASE DIAGNOSTIC] Required diagnostic query failed."
    );
    format!("{operation} unavailable")
}

fn dependency_unavailable(operation: &'static str) -> String {
    format!("{operation} unavailable: database connection failed")
}

pub async fn collect(pool: &PgPool) -> AdminDatabaseSnapshot {
    let connection = sqlx::query_scalar::<_, i64>("SELECT 1::BIGINT")
        .fetch_one(pool)
        .await
        .and_then(|value| {
            if value == 1 {
                Ok(value)
            } else {
                Err(sqlx::Error::Protocol(
                    "unexpected database ping result".to_string(),
                ))
            }
        })
        .map(|_| ())
        .map_err(|error| query_error("connection_ping", error));

    if connection.is_err() {
        return AdminDatabaseSnapshot {
            connection,
            users_count: Err(dependency_unavailable("users_count")),
            wallets_count: Err(dependency_unavailable("wallets_count")),
            mined_count: Err(dependency_unavailable("mined_count")),
            settings_count: Err(dependency_unavailable("settings_count")),
            last_alert: Err(dependency_unavailable("last_alert")),
        };
    }

    let users_count =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(DISTINCT chat_id) FROM user_wallets")
            .fetch_one(pool)
            .await
            .map_err(|error| query_error("users_count", error));
    let wallets_count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM user_wallets")
        .fetch_one(pool)
        .await
        .map_err(|error| query_error("wallets_count", error));
    let mined_count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM mined_blocks")
        .fetch_one(pool)
        .await
        .map_err(|error| query_error("mined_count", error));
    let settings_count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM system_settings")
        .fetch_one(pool)
        .await
        .map_err(|error| query_error("settings_count", error));
    let last_alert =
        sqlx::query_scalar::<_, Option<DateTime<Utc>>>("SELECT MAX(timestamp) FROM mined_blocks")
            .fetch_one(pool)
            .await
            .map_err(|error| query_error("last_alert", error));

    AdminDatabaseSnapshot {
        connection,
        users_count,
        wallets_count,
        mined_count,
        settings_count,
        last_alert,
    }
}

pub fn display_count(value: &Result<i64, String>) -> String {
    match value {
        Ok(value) => value.to_string(),
        Err(_) => "UNAVAILABLE".to_string(),
    }
}

pub fn display_last_alert(value: &Result<Option<DateTime<Utc>>, String>) -> String {
    match value {
        Ok(Some(timestamp)) => timestamp.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
        Ok(None) => "No alerts yet".to_string(),
        Err(_) => "UNAVAILABLE".to_string(),
    }
}
