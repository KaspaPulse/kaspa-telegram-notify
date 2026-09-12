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

// Keep the complete encoded HTML below Telegram's limit as well as the rendered
// text. Budget UTF-16 units so supplementary Unicode characters remain safe.
const RESPONSE_MAX_UTF16_UNITS: usize = 4_000;
const RESPONSE_HEADER: &str = "📜 <b>Recent Service Logs</b>\n<pre>";
const RESPONSE_FOOTER: &str = "</pre>";
const RESPONSE_OMITTED: &str = "[Earlier log content omitted]\n";

pub fn response_html(lines: &[String]) -> String {
    if lines.is_empty() {
        return format!(
            "{RESPONSE_HEADER}No recent in-process service logs are available yet.{RESPONSE_FOOTER}"
        );
    }

    let overhead = [RESPONSE_HEADER, RESPONSE_FOOTER, RESPONSE_OMITTED]
        .iter()
        .map(|part| part.encode_utf16().count())
        .sum::<usize>();
    let mut remaining = RESPONSE_MAX_UTF16_UNITS - overhead;
    let mut selected = Vec::new();
    let mut omitted = false;
    for line in lines.iter().rev() {
        let safe = crate::utils::html_escape(line);
        let units = safe.encode_utf16().count();
        let separator = usize::from(!selected.is_empty());
        if units + separator <= remaining {
            remaining -= units + separator;
            selected.push(safe);
        } else {
            // Preserve at least part of the newest event even when one line
            // alone exceeds the response budget. Escape complete characters.
            if selected.is_empty() {
                let mut partial = String::new();
                for character in line.chars() {
                    let escaped = crate::utils::html_escape(&character.to_string());
                    let units = escaped.encode_utf16().count();
                    if units + 1 > remaining {
                        break;
                    }
                    remaining -= units;
                    partial.push_str(&escaped);
                }
                partial.push('…');
                selected.push(partial);
            }
            omitted = true;
            break;
        }
    }
    selected.reverse();
    let notice = if omitted { RESPONSE_OMITTED } else { "" };
    format!(
        "{RESPONSE_HEADER}{notice}{}{RESPONSE_FOOTER}",
        selected.join("\n")
    )
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
    fn f06_log_response_bounds_long_events_and_keeps_newest_in_order() {
        let lines = (0..25)
            .map(|index| format!("event-{index:02}: {}", "x".repeat(1_000)))
            .collect::<Vec<_>>();
        // This is the old handler's message payload: it violates Telegram's limit.
        let previous = format!("{RESPONSE_HEADER}{}{RESPONSE_FOOTER}", lines.join("\n"));
        assert!(previous.encode_utf16().count() > 4_096);

        let response = response_html(&lines);
        assert!(response.encode_utf16().count() <= RESPONSE_MAX_UTF16_UNITS);
        assert!(response.contains(RESPONSE_OMITTED));
        assert!(!response.contains("event-00:"));
        let previous_event = response.find("event-23:").unwrap();
        let newest_event = response.find("event-24:").unwrap();
        assert!(previous_event < newest_event);
        assert!(response.ends_with(RESPONSE_FOOTER));
    }

    #[test]
    fn f06_log_response_truncates_unicode_without_broken_html_entities() {
        let line = format!("newest: {}", "🧪&<>'\"".repeat(1_000));
        let response = response_html(&[line]);
        assert!(response.encode_utf16().count() <= RESPONSE_MAX_UTF16_UNITS);
        assert!(response.contains("newest: 🧪&amp;&lt;&gt;&#39;&quot;"));
        assert!(response.ends_with("…</pre>"));
        let body = response
            .strip_prefix(RESPONSE_HEADER)
            .unwrap()
            .strip_suffix(RESPONSE_FOOTER)
            .unwrap();
        assert!(!body.contains('<'));
        assert!(!body.contains('>'));
        // Every encoded ampersand must begin a complete entity.
        for entity in body.split('&').skip(1) {
            assert!(
                ["amp;", "lt;", "gt;", "quot;", "#39;"]
                    .iter()
                    .any(|known| entity.starts_with(known))
            );
        }
    }

    #[test]
    fn f06_log_response_preserves_short_events_and_explicit_empty_state() {
        let response = response_html(&["first <event>".to_string(), "second 🧪".to_string()]);
        assert_eq!(
            response,
            format!("{RESPONSE_HEADER}first &lt;event&gt;\nsecond 🧪{RESPONSE_FOOTER}")
        );
        let empty = response_html(&[]);
        assert!(empty.contains("No recent in-process service logs are available yet."));
        assert!(!empty.contains(RESPONSE_OMITTED));
    }

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
