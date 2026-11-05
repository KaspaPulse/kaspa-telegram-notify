use kaspa_pulse::wallet::alert_delivery_gate::{ALERT_DELIVERY_SETTING_KEY, parse_enabled_value};

#[test]
fn alert_delivery_setting_key_is_stable() {
    assert_eq!(ALERT_DELIVERY_SETTING_KEY, "ENABLE_ALERT_DELIVERY");
}

#[test]
fn parses_alert_delivery_enabled_values() {
    assert!(parse_enabled_value("true"));
    assert!(parse_enabled_value("1"));
    assert!(parse_enabled_value("enabled"));
    assert!(!parse_enabled_value("false"));
    assert!(!parse_enabled_value("0"));
}
