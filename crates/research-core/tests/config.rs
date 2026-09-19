use research_core::Config;

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
