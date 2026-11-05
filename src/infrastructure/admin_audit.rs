use crate::domain::errors::AppError;
use sqlx::PgPool;

pub fn sanitize_action_name(action: &str) -> String {
    action
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-' || *c == ':')
        .take(80)
        .collect()
}

pub async fn record_admin_action(
    pool: &PgPool,
    admin_actor_user_id: u64,
    admin_chat_id: i64,
    action: &str,
    old_value: Option<&str>,
    new_value: Option<&str>,
    status: &str,
) -> Result<(), AppError> {
    let actor_user_id = i64::try_from(admin_actor_user_id)
        .map_err(|_| AppError::Internal("Telegram actor ID is out of range.".to_string()))?;
    let action = sanitize_action_name(action);

    sqlx::query(
        "INSERT INTO admin_audit_log
         (admin_actor_user_id, admin_chat_id, action, old_value, new_value, status)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(actor_user_id)
    .bind(admin_chat_id)
    .bind(action)
    .bind(old_value)
    .bind(new_value)
    .bind(status)
    .execute(pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_admin_action_name() {
        assert_eq!(sanitize_action_name("mute_alerts"), "mute_alerts");
        assert_eq!(sanitize_action_name("bad action <>"), "badaction");
    }
}
