//! Dispatcher-owned request work also belongs to the runtime DB lifetime.
use super::*;

async fn owned<F>(name: &'static str, future: F) -> anyhow::Result<()>
where
    F: std::future::Future<Output = anyhow::Result<()>> + Send + 'static,
{
    let Some(task) = crate::infrastructure::resilience::runtime::spawn_tracked(name, future) else {
        return Ok(());
    };
    task.join().await?
}

pub async fn handle_command(
    bot: Bot,
    msg: Message,
    cmd: Command,
    ucs: BotUseCases,
    app_context: Arc<crate::domain::models::AppContext>,
) -> anyhow::Result<()> {
    owned(
        "telegram_command",
        super::handle_command(bot, msg, cmd, ucs, app_context),
    )
    .await
}

pub async fn handle_callback(
    bot: Bot,
    q: teloxide::types::CallbackQuery,
    ucs: BotUseCases,
    app_context: Arc<crate::domain::models::AppContext>,
    callback_execution_registry: Arc<
        crate::presentation::telegram::callback_inflight::CallbackExecutionRegistry,
    >,
) -> anyhow::Result<()> {
    owned(
        "telegram_callback",
        super::handle_callback(bot, q, ucs, app_context, callback_execution_registry),
    )
    .await
}

pub async fn handle_raw_message(
    bot: Bot,
    msg: Message,
    app_context: Arc<crate::domain::models::AppContext>,
) -> anyhow::Result<()> {
    owned(
        "telegram_raw_message",
        super::handle_raw_message(bot, msg, app_context),
    )
    .await
}
