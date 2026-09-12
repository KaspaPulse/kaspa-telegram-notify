use std::collections::VecDeque;
use std::fmt::Write as _;
use std::sync::{Mutex, OnceLock};

use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;

const DEFAULT_BUFFER_LINES: usize = 256;
const DEFAULT_LINE_CHARS: usize = 1_000;

fn buffer() -> &'static Mutex<VecDeque<String>> {
    static BUFFER: OnceLock<Mutex<VecDeque<String>>> = OnceLock::new();
    BUFFER.get_or_init(|| Mutex::new(VecDeque::new()))
}

fn max_lines() -> usize {
    crate::utils::env_usize("ADMIN_LOG_BUFFER_LINES", DEFAULT_BUFFER_LINES).clamp(25, 2_000)
}

fn max_line_chars() -> usize {
    crate::utils::env_usize("ADMIN_LOG_LINE_CHARS", DEFAULT_LINE_CHARS).clamp(200, 4_000)
}

fn store_line(raw: String) {
    let sanitized = crate::utils::sanitize_for_log(&raw);
    let max_chars = max_line_chars();
    let line = if sanitized.chars().count() > max_chars {
        format!("{}…", sanitized.chars().take(max_chars).collect::<String>())
    } else {
        sanitized
    };

    let Ok(mut guard) = buffer().lock() else {
        return;
    };
    guard.push_back(line);
    let capacity = max_lines();
    while guard.len() > capacity {
        guard.pop_front();
    }
}

pub fn recent_lines(limit: usize) -> Vec<String> {
    let Ok(guard) = buffer().lock() else {
        return Vec::new();
    };
    let limit = limit.clamp(1, 100).min(guard.len());
    guard
        .iter()
        .skip(guard.len().saturating_sub(limit))
        .cloned()
        .collect()
}

#[derive(Default)]
struct FieldVisitor {
    rendered: String,
}

impl Visit for FieldVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if !self.rendered.is_empty() {
            self.rendered.push(' ');
        }
        if field.name() == "message" {
            let _ = write!(self.rendered, "{value:?}");
        } else {
            let _ = write!(self.rendered, "{}={value:?}", field.name());
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RecentLogLayer;

pub fn layer() -> RecentLogLayer {
    RecentLogLayer
}

impl<S> Layer<S> for RecentLogLayer
where
    S: Subscriber,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let metadata = event.metadata();
        let mut visitor = FieldVisitor::default();
        event.record(&mut visitor);
        let line = format!(
            "{} {} {} {}",
            chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            metadata.level(),
            metadata.target(),
            visitor.rendered
        );
        store_line(line);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f06_recent_logs_are_bounded_and_sanitized() {
        {
            let mut guard = buffer().lock().unwrap();
            guard.clear();
        }
        store_line(
            "BOT_TOKEN=secret-value kaspa:qabcdefghijklmnopqrstuvwxyz1234567890abcdef".into(),
        );
        let lines = recent_lines(25);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("***REDACTED***"));
        assert!(!lines[0].contains("secret-value"));
        assert!(!lines[0].contains("qabcdefghijklmnopqrstuvwxyz1234567890abcdef"));
    }
}
