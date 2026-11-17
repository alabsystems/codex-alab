// Copyright 2026 Andrew Yates
// SPDX-License-Identifier: Apache-2.0

use pretty_assertions::assert_eq;

#[test]
fn kimi_profiles_are_opt_in_and_use_subscription_api_keys() {
    let defaults: toml::Table = toml::from_str(crate::ALAB_DEFAULTS).expect("ALab defaults");
    assert_eq!(defaults["model"].as_str(), Some("gpt-6.2-sol"));
    assert_eq!(defaults["model_reasoning_effort"].as_str(), Some("ultra"));
    assert_eq!(defaults.get("model_provider"), None);

    let providers = defaults["model_providers"].as_table().expect("providers");
    for (id, base_url) in [
        ("kimi", "https://api.kimi.ai/coding/v1"),
        ("kimi-cn", "https://api.kimi.com/coding/v1"),
    ] {
        let provider = providers[id].as_table().expect("Kimi provider");
        assert_eq!(provider["base_url"].as_str(), Some(base_url));
        assert_eq!(provider["env_key"].as_str(), Some("KIMI_API_KEY"));
        assert_eq!(provider["wire_api"].as_str(), Some("responses"));
        assert_eq!(provider.get("requires_openai_auth"), None);
        assert!(
            provider["env_key_instructions"]
                .as_str()
                .is_some_and(|instructions| instructions.contains("Kimi Code Console"))
        );
    }

    for id in ["kimi", "kimi-cn"] {
        let profile = defaults["profiles"][id].as_table().expect("Kimi profile");
        assert_eq!(profile["model"].as_str(), Some("k3-256k"));
        assert_eq!(profile["model_provider"].as_str(), Some(id));
        assert_eq!(profile["model_reasoning_effort"].as_str(), Some("high"));
        assert_eq!(profile["web_search"].as_str(), Some("live"));
        assert_eq!(
            profile["tools"]["update_plan"]["enabled"].as_bool(),
            Some(true)
        );
        assert_eq!(profile.get("model_catalog_json"), None);
    }
}

#[test]
fn selected_packaged_profiles_overlay_defaults_without_changing_the_base() {
    let defaults = crate::alab_defaults(crate::UPSTREAM_DEFAULTS, false).expect("ALab defaults");
    let base: toml::Table = toml::from_str(&defaults).expect("base defaults");
    assert_eq!(base["model"].as_str(), Some("gpt-6.2-sol"));
    assert_eq!(base["model_reasoning_effort"].as_str(), Some("ultra"));
    assert_eq!(base.get("model_provider"), None);

    for id in ["kimi", "kimi-cn"] {
        let selected = crate::packaged_profile_defaults(id).expect("selected defaults");
        assert_eq!(selected["model"].as_str(), Some("k3-256k"));
        assert_eq!(selected["model_provider"].as_str(), Some(id));
        assert_eq!(selected["web_search"].as_str(), Some("live"));
        assert_eq!(selected["model_reasoning_effort"].as_str(), Some("high"));

        let mut merged = base.clone();
        crate::merge_tables(&mut merged, selected);
        assert_eq!(merged["model"].as_str(), Some("k3-256k"));
        assert_eq!(merged["model_provider"].as_str(), Some(id));
        assert_eq!(merged["model_reasoning_effort"].as_str(), Some("high"));
    }

    assert!(crate::packaged_profile_defaults("other").is_none());
    let unchanged: toml::Table = toml::from_str(&defaults).expect("unchanged defaults");
    assert_eq!(unchanged, base);
}
