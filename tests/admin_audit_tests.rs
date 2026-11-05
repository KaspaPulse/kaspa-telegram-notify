use kaspa_pulse::infrastructure::admin_audit::sanitize_action_name;

#[test]
fn admin_action_name_is_sanitized() {
    assert_eq!(sanitize_action_name("mute_alerts"), "mute_alerts");
    assert_eq!(sanitize_action_name("bad action <>"), "badaction");
}
