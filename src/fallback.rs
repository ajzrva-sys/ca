use crate::agent::stream::{ProviderError, ProviderTransportFailure};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ProviderFallbackCause {
    Connect,
    Timeout,
    RateLimited,
    ServerError,
}

impl std::fmt::Display for ProviderFallbackCause {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Connect => "connect",
            Self::Timeout => "timeout",
            Self::RateLimited => "rate limited",
            Self::ServerError => "server error",
        })
    }
}

pub fn default_fallback_causes() -> Vec<ProviderFallbackCause> {
    vec![
        ProviderFallbackCause::Connect,
        ProviderFallbackCause::Timeout,
        ProviderFallbackCause::RateLimited,
        ProviderFallbackCause::ServerError,
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProviderFallbackRule {
    pub provider: String,
    pub model: String,
    pub fallback_provider: String,
    pub fallback_model: String,
    #[serde(default = "default_fallback_causes")]
    pub causes: Vec<ProviderFallbackCause>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TurnProviderProvenance {
    pub user_message_index: usize,
    pub primary_provider: String,
    pub primary_model: String,
    pub provider: String,
    pub model: String,
    pub cause: ProviderFallbackCause,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRoute {
    pub provider: String,
    pub model: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnProviderRoute {
    pub primary: ProviderRoute,
    pub active: ProviderRoute,
    pub fallback_used: bool,
}

impl TurnProviderRoute {
    pub fn primary(provider: impl Into<String>, model: impl Into<String>) -> Self {
        let primary = ProviderRoute {
            provider: provider.into(),
            model: model.into(),
        };
        Self {
            active: primary.clone(),
            primary,
            fallback_used: false,
        }
    }
}

pub fn cause_for_error(error: &ProviderError) -> Option<ProviderFallbackCause> {
    match error {
        ProviderError::Transport(ProviderTransportFailure::Connect) => {
            Some(ProviderFallbackCause::Connect)
        }
        ProviderError::Transport(ProviderTransportFailure::Timeout)
        | ProviderError::Transport(ProviderTransportFailure::FirstTokenTimeout) => {
            Some(ProviderFallbackCause::Timeout)
        }
        ProviderError::Http { status: 429, .. } => Some(ProviderFallbackCause::RateLimited),
        ProviderError::Http {
            status: 500..=599, ..
        } => Some(ProviderFallbackCause::ServerError),
        _ => None,
    }
}

pub fn matching_rule<'a>(
    rules: &'a [ProviderFallbackRule],
    provider: &str,
    model: &str,
    error: &ProviderError,
) -> Option<(&'a ProviderFallbackRule, ProviderFallbackCause)> {
    let cause = cause_for_error(error)?;
    rules
        .iter()
        .find(|rule| {
            rule.provider == provider && rule.model == model && rule.causes.contains(&cause)
        })
        .map(|rule| (rule, cause))
}

pub fn validate_rules(config: &crate::config::CaConfig) -> anyhow::Result<()> {
    let mut primaries = HashSet::new();
    for rule in &config.fallbacks {
        validate_component("fallback primary provider", &rule.provider)?;
        validate_component("fallback primary model", &rule.model)?;
        validate_component("fallback provider", &rule.fallback_provider)?;
        validate_component("fallback model", &rule.fallback_model)?;
        let primary = (&rule.provider, &rule.model);
        if !primaries.insert(primary) {
            anyhow::bail!(
                "duplicate fallback rule for {}/{}",
                rule.provider,
                rule.model
            );
        }
        if rule.provider == rule.fallback_provider && rule.model == rule.fallback_model {
            anyhow::bail!(
                "fallback rule for {}/{} points to itself",
                rule.provider,
                rule.model
            );
        }
        if rule.causes.is_empty() {
            anyhow::bail!(
                "fallback rule for {}/{} must allow at least one cause",
                rule.provider,
                rule.model
            );
        }
        let mut causes = HashSet::new();
        for cause in &rule.causes {
            if !causes.insert(*cause) {
                anyhow::bail!(
                    "fallback rule for {}/{} repeats cause {cause}",
                    rule.provider,
                    rule.model
                );
            }
        }
        validate_exact_model(config, &rule.provider, &rule.model, "primary")?;
        validate_exact_model(
            config,
            &rule.fallback_provider,
            &rule.fallback_model,
            "fallback",
        )?;
    }
    Ok(())
}

fn validate_component(label: &str, value: &str) -> anyhow::Result<()> {
    if value.is_empty() || value.trim() != value {
        anyhow::bail!("{label} must be a non-empty exact identifier");
    }
    Ok(())
}

fn validate_exact_model(
    config: &crate::config::CaConfig,
    provider: &str,
    model: &str,
    label: &str,
) -> anyhow::Result<()> {
    let Some(provider_config) = config.providers.get(provider) else {
        anyhow::bail!("{label} fallback provider {provider} is not configured");
    };
    if !provider_config.models.iter().any(|entry| entry == model) {
        anyhow::bail!("{label} fallback model {provider}/{model} is not configured");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_with_rules(rules: Vec<ProviderFallbackRule>) -> crate::config::CaConfig {
        let mut config = crate::config::CaConfig::default();
        for (provider, model) in [("primary", "one"), ("backup", "two")] {
            config.providers.insert(
                provider.into(),
                crate::config::ProviderConfig {
                    google: None,
                    tool_schema_policy: Default::default(),
                    base_url: format!("https://{provider}.invalid/v1"),
                    api_key: String::new(),
                    protocol: crate::agent::ProviderProtocol::default(),
                    prompt_cache_mode: crate::config::PromptCacheMode::Implicit,
                    models: vec![model.into()],
                    timeout: crate::config::ProviderTimeoutConfig::default(),
                },
            );
        }
        config.fallbacks = rules;
        config
    }

    fn rule() -> ProviderFallbackRule {
        ProviderFallbackRule {
            provider: "primary".into(),
            model: "one".into(),
            fallback_provider: "backup".into(),
            fallback_model: "two".into(),
            causes: default_fallback_causes(),
        }
    }

    #[test]
    fn only_closed_safe_failures_are_classified() {
        for (error, expected) in [
            (
                ProviderError::Transport(ProviderTransportFailure::Connect),
                Some(ProviderFallbackCause::Connect),
            ),
            (
                ProviderError::Transport(ProviderTransportFailure::Timeout),
                Some(ProviderFallbackCause::Timeout),
            ),
            (
                ProviderError::Http {
                    status: 429,
                    body_excerpt: None,
                },
                Some(ProviderFallbackCause::RateLimited),
            ),
            (
                ProviderError::Http {
                    status: 503,
                    body_excerpt: None,
                },
                Some(ProviderFallbackCause::ServerError),
            ),
            (
                ProviderError::Http {
                    status: 401,
                    body_excerpt: None,
                },
                None,
            ),
            (
                ProviderError::Http {
                    status: 403,
                    body_excerpt: None,
                },
                None,
            ),
            (
                ProviderError::Http {
                    status: 404,
                    body_excerpt: None,
                },
                None,
            ),
            (
                ProviderError::Transport(ProviderTransportFailure::Request),
                None,
            ),
            (ProviderError::ResponseRefused, None),
            (ProviderError::Protocol("bad".into()), None),
        ] {
            assert_eq!(cause_for_error(&error), expected);
        }
    }

    #[test]
    fn matching_is_exact_and_cause_bounded() {
        let mut selected = rule();
        selected.causes = vec![ProviderFallbackCause::Timeout];
        let rules = vec![selected];
        assert!(matching_rule(
            &rules,
            "primary",
            "one",
            &ProviderError::Transport(ProviderTransportFailure::Timeout)
        )
        .is_some());
        assert!(matching_rule(
            &rules,
            "primary",
            "other",
            &ProviderError::Transport(ProviderTransportFailure::Timeout)
        )
        .is_none());
        assert!(matching_rule(
            &rules,
            "primary",
            "one",
            &ProviderError::Http {
                status: 429,
                body_excerpt: None
            }
        )
        .is_none());
    }

    #[test]
    fn rules_require_unique_exact_configured_pairs() {
        let valid = config_with_rules(vec![rule()]);
        validate_rules(&valid).unwrap();

        let mut duplicate = rule();
        duplicate.fallback_model = "two".into();
        let error = validate_rules(&config_with_rules(vec![rule(), duplicate]))
            .unwrap_err()
            .to_string();
        assert!(error.contains("duplicate fallback rule"));

        let mut repeated = rule();
        repeated.causes = vec![
            ProviderFallbackCause::Timeout,
            ProviderFallbackCause::Timeout,
        ];
        assert!(validate_rules(&config_with_rules(vec![repeated]))
            .unwrap_err()
            .to_string()
            .contains("repeats cause"));

        let mut missing = rule();
        missing.fallback_model = "missing".into();
        assert!(validate_rules(&config_with_rules(vec![missing]))
            .unwrap_err()
            .to_string()
            .contains("is not configured"));

        let mut self_route = rule();
        self_route.fallback_provider = "primary".into();
        self_route.fallback_model = "one".into();
        assert!(validate_rules(&config_with_rules(vec![self_route]))
            .unwrap_err()
            .to_string()
            .contains("points to itself"));
    }

    #[test]
    fn config_load_defaults_causes_and_rejects_invalid_rules_before_runtime() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        std::fs::write(
            &path,
            r#"[providers.primary]
base_url = "https://primary.invalid/v1"
api_key = ""
models = ["one"]

[providers.backup]
base_url = "https://backup.invalid/v1"
api_key = ""
models = ["two"]

[[fallbacks]]
provider = "primary"
model = "one"
fallback_provider = "backup"
fallback_model = "two"
"#,
        )
        .unwrap();
        let loaded = crate::config::ConfigStore::new(&path).load().unwrap();
        assert_eq!(loaded.fallbacks[0].causes, default_fallback_causes());

        let original = std::fs::read_to_string(&path).unwrap();
        let invalid = original.replace(
            "fallback_model = \"two\"",
            "fallback_model = \"two\"\ncauses = [\"authentication\"]",
        );
        std::fs::write(&path, invalid).unwrap();
        let error = crate::config::ConfigStore::new(&path)
            .load()
            .unwrap_err()
            .to_string();
        assert!(error.contains("unknown variant") || error.contains("authentication"));

        let empty = original.replace(
            "fallback_model = \"two\"",
            "fallback_model = \"two\"\ncauses = []",
        );
        std::fs::write(&path, empty).unwrap();
        assert!(crate::config::ConfigStore::new(&path)
            .load()
            .unwrap_err()
            .to_string()
            .contains("at least one cause"));
    }
}
