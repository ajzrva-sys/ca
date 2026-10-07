use reasoning_pilot_config_migration::{load_config, Config};

#[test]
fn current_config_loads() {
    let input = "version=2\nprovider_url=https://provider.invalid/v1\nmodel=test-model\ntimeout_seconds=12\n";
    assert_eq!(
        load_config(input).unwrap(),
        Config {
            provider_url: "https://provider.invalid/v1".into(),
            model: "test-model".into(),
            timeout_seconds: 12,
        }
    );
}
