use kaspa_pulse::infrastructure::metrics::render_metrics;

#[test]
fn metrics_render_contains_expected_counters() {
    let text = render_metrics();

    assert!(text.contains("kaspa_pulse_alerts_delivered_total"));
    assert!(text.contains("kaspa_pulse_alerts_suppressed_total"));
    assert!(text.contains("kaspa_pulse_admin_actions_confirmed_total"));
    assert!(text.contains("kaspa_pulse_build_info{version=\""));
    assert!(text.contains("source_revision=\""));
}
