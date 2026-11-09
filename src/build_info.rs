//! Immutable provenance of the *running executable*, not a mutable GitHub branch.
//! OP092: labels are build-time attested; process started-at is captured once.
//! All owners' detail queries are local-only. No RPC, DB or Telegram writes.
use chrono::{DateTime, FixedOffset, Utc};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::sync::OnceLock;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const COMPILED_SOURCE_REVISION: Option<&str> = option_env!("KASPA_PULSE_SOURCE_REVISION");
pub const COMPILED_GIT_COMMIT: Option<&str> = option_env!("KASPA_PULSE_GIT_COMMIT");
pub const COMPILED_COMMIT_UTC: Option<&str> = option_env!("KASPA_PULSE_GIT_COMMIT_UTC");
pub const COMPILED_BUILD_UTC: Option<&str> = option_env!("KASPA_PULSE_BUILD_UTC");
pub const COMPILED_SOURCE_TREE_SHA256: Option<&str> = option_env!("KASPA_PULSE_SOURCE_TREE_SHA256");
static STARTED_UTC: OnceLock<DateTime<Utc>> = OnceLock::new();
static RUNNING_BINARY_SHA256: OnceLock<String> = OnceLock::new();

pub fn mark_process_started() {
    STARTED_UTC.get_or_init(Utc::now);
}

pub fn source_revision() -> &'static str {
    COMPILED_SOURCE_REVISION
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown")
}

fn known(value: Option<&'static str>) -> &'static str {
    value.filter(|v| !v.is_empty()).unwrap_or("not recorded")
}

pub fn git_commit() -> &'static str {
    known(COMPILED_GIT_COMMIT)
}

pub fn build_utc() -> &'static str {
    known(COMPILED_BUILD_UTC)
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

fn sha256_executing_binary() -> String {
    let Ok(mut file) = std::fs::File::open("/proc/self/exe") else {
        return "unavailable".into();
    };
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        match file.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => hasher.update(&buf[..n]),
            Err(_) => return "unavailable".into(),
        }
    }
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn running_binary_sha256() -> &'static str {
    RUNNING_BINARY_SHA256.get_or_init(sha256_executing_binary)
}

fn date_riyadh(value: &str) -> String {
    let Ok(parsed) = DateTime::parse_from_rfc3339(value) else {
        return "not recorded".into();
    };
    let offset = FixedOffset::east_opt(3 * 3600).expect("Riyadh UTC+3 is fixed");
    parsed
        .with_timezone(&offset)
        .format("%Y-%m-%d %H:%M:%S UTC+03")
        .to_string()
}

fn html(value: &str) -> String {
    crate::utils::html_escape(value)
}

/// A short public card and a more detailed owner-only card. The owner is
/// authenticated upstream using both private chat and Telegram sender ID.
pub fn release_card(
    lang: crate::presentation::telegram::menus::Language,
    is_admin: bool,
) -> String {
    use crate::presentation::telegram::menus::Language;
    let ar = lang == Language::Ar;
    let title = if ar {
        "📦 <b>الإصدار العامل في الإنتاج</b>"
    } else {
        "📦 <b>Running Production Release</b>"
    };
    let short_commit = if git_commit().len() >= 12 {
        &git_commit()[..12]
    } else {
        git_commit()
    };
    let mut card = format!(
        "{}\n\n{} <code>{}</code>\n{} <code>{}</code>\n{} <code>{}</code>\n{} <code>{}</code>\n{} <code>{}</code>",
        title,
        if ar {
            "إصدار البرنامج:"
        } else {
            "Application version:"
        },
        html(VERSION),
        if ar {
            "إصدار الإنتاج الحي:"
        } else {
            "Live production release:"
        },
        html(source_revision()),
        if ar {
            "التزام Git:"
        } else {
            "Git commit:"
        },
        html(short_commit),
        if ar {
            "وقت بناء الإصدار (الرياض):"
        } else {
            "Build time (Riyadh):"
        },
        html(&date_riyadh(build_utc())),
        if ar {
            "بدأ تشغيل الإنتاج (الرياض):"
        } else {
            "Production process started (Riyadh):"
        },
        html(
            &STARTED_UTC
                .get()
                .map(|x| date_riyadh(&x.to_rfc3339()))
                .unwrap_or_else(|| "not recorded".into())
        ),
    );
    if is_admin {
        card.push_str(&format!(
            "\n\n{} <code>{}</code>\n{} <code>{}</code>\n{} <code>{}</code>\n{} <code>{}</code>\n{} <code>{}</code>",
            if ar { "الالتزام الكامل:" } else { "Full Git commit:" }, html(git_commit()),
            if ar { "تاريخ الالتزام (الرياض):" } else { "Commit date (Riyadh):" },
            html(&date_riyadh(known(COMPILED_COMMIT_UTC))),
            if ar { "بصمة المصدر SHA-256:" } else { "Source SHA-256:" },
            html(known(COMPILED_SOURCE_TREE_SHA256)),
            if ar { "بصمة الملف التنفيذي SHA-256:" } else { "Running binary SHA-256:" },
            html(running_binary_sha256()),
            if ar { "البيئة:" } else { "Environment:" },
            if ar { "الإنتاج — الملف التنفيذي الفعلي" } else { "Production — actual running executable" }
        ));
        card.push_str(if ar {
            "\n\n🔒 المعلومات مأخوذة من الملف التنفيذي العامل، لا من آخر نسخة بالمستودع."
        } else {
            "\n\n🔒 Identity is read from the running executable, not the latest repository branch."
        });
    }
    card
}

pub fn render_prometheus() -> String {
    format!(
        concat!(
            "# HELP kaspa_pulse_build_info Build identity for the running Kaspa Pulse binary.\n",
            "# TYPE kaspa_pulse_build_info gauge\n",
            "kaspa_pulse_build_info{{version=\"{}\",source_revision=\"{}\",git_commit=\"{}\",source_sha256=\"{}\",built_utc=\"{}\",binary_sha256=\"{}\"}} 1\n"
        ),
        escape_prometheus_label(VERSION),
        escape_prometheus_label(source_revision()),
        escape_prometheus_label(git_commit()),
        escape_prometheus_label(known(COMPILED_SOURCE_TREE_SHA256)),
        escape_prometheus_label(build_utc()),
        escape_prometheus_label(running_binary_sha256()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presentation::telegram::menus::Language;
    #[test]
    fn labels_escape_special_characters() {
        assert_eq!(escape_prometheus_label("a\\b\"c\nd"), "a\\\\b\\\"c\\nd");
    }
    #[test]
    fn build_info_metric_contains_version_revision_and_real_binary_hash() {
        let rendered = render_prometheus();
        assert!(rendered.contains(&format!("version=\"{}\"", VERSION)));
        assert!(rendered.contains(&format!("source_revision=\"{}\"", source_revision())));
        assert!(rendered.contains("git_commit="));
        assert!(rendered.contains("built_utc="));
        assert_eq!(running_binary_sha256().len(), 64);
    }
    #[test]
    fn release_cards_are_bilingual_and_owner_details_private() {
        let en = release_card(Language::En, false);
        let ar = release_card(Language::Ar, false);
        let owner = release_card(Language::Ar, true);
        assert!(en.contains("Running Production Release"));
        assert!(ar.contains("الإصدار العامل في الإنتاج"));
        assert!(en.contains(source_revision()));
        assert!(ar.contains(source_revision()));
        assert!(!en.contains("Running binary SHA-256:"));
        assert!(!ar.contains("بصمة الملف التنفيذي SHA-256:"));
        assert!(owner.contains("بصمة الملف التنفيذي SHA-256:"));
        assert!(owner.contains("الالتزام الكامل:"));
        assert!(!owner.contains("kaspa:q"));
    }
    #[test]
    fn riyadh_offset_never_uses_local_timezone_or_fabricates_dates() {
        assert_eq!(
            date_riyadh("2026-11-09T07:00:00Z"),
            "2026-11-09 10:00:00 UTC+03"
        );
        assert_eq!(date_riyadh("not-a-timestamp"), "not recorded");
    }
}
