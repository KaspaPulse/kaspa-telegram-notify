pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const COMPILED_SOURCE_REVISION: Option<&str> = option_env!("KASPA_PULSE_SOURCE_REVISION");

pub fn source_revision() -> &'static str {
    match COMPILED_SOURCE_REVISION {
        Some(value) if !value.is_empty() => value,
        _ => "unknown",
    }
}

fn escape_prometheus_label(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            other => escaped.push(other),
        }
    }
    escaped
}

pub fn render_prometheus() -> String {
    format!(
        concat!(
            "# HELP kaspa_pulse_build_info Build identity for the running Kaspa Pulse binary.\n",
            "# TYPE kaspa_pulse_build_info gauge\n",
            "kaspa_pulse_build_info{{version=\"{}\",source_revision=\"{}\"}} 1\n"
        ),
        escape_prometheus_label(VERSION),
        escape_prometheus_label(source_revision()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prometheus_labels_escape_special_characters() {
        assert_eq!(escape_prometheus_label("a\\b\"c\nd"), "a\\\\b\\\"c\\nd");
    }

    #[test]
    fn build_info_metric_contains_version_and_revision() {
        let rendered = render_prometheus();
        assert!(rendered.contains(&format!("version=\"{}\"", VERSION)));
        assert!(rendered.contains(&format!("source_revision=\"{}\"", source_revision())));
    }
}
