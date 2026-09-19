use research_core::Config;

#[test]
fn browser_idle_defaults_are_backward_compatible_and_validated() {
    let mut config: Config = serde_json::from_str("{}").unwrap();
    assert!(config.chrome_auto_close);
    assert_eq!(config.chrome_idle_timeout_minutes, 30);
    let saved = serde_json::to_value(&config).unwrap();
    assert!(saved.get("chrome_auto_close").is_none());
    assert!(saved.get("chrome_idle_timeout_minutes").is_none());
    config.chrome_idle_timeout_minutes = 0;
    assert!(config.validate().is_err());
    config.chrome_idle_timeout_minutes = 1;
    config.chrome_auto_close = false;
    config.validate().unwrap();
    let saved = serde_json::to_value(config).unwrap();
    assert_eq!(saved["chrome_auto_close"], false);
    assert_eq!(saved["chrome_idle_timeout_minutes"], 1);
}

#[test]
fn retention_names_accept_legacy_configs_and_write_canonical_keys() {
    let legacy: Config =
        serde_json::from_str(r#"{"inactivity_hours":48,"transcript_retention_days":60}"#).unwrap();
    let current: Config = serde_json::from_str(
        r#"{"remote_chat_inactivity_hours":48,"local_transcript_retention_days":60}"#,
    )
    .unwrap();
    legacy.validate().unwrap();
    let saved = serde_json::to_value(&legacy).unwrap();
    assert_eq!(saved, serde_json::to_value(current).unwrap());
    assert_eq!(saved["remote_chat_inactivity_hours"], 48);
    assert_eq!(saved["local_transcript_retention_days"], 60);
    assert!(saved.get("inactivity_hours").is_none());
    assert!(saved.get("transcript_retention_days").is_none());
}
