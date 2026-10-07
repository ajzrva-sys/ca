use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub provider_url: String,
    pub model: String,
    pub timeout_seconds: u64,
}

pub fn load_config(input: &str) -> Result<Config, String> {
    let fields = parse_fields(input)?;
    match required(&fields, "version")? {
        "1" => {
            reject_unknown(&fields, &["version", "api_base", "model"])?;
            Ok(Config {
                provider_url: required(&fields, "api_base")?.to_string(),
                model: required(&fields, "model")?.to_string(),
                timeout_seconds: 30,
            })
        }
        "2" => {
            reject_unknown(
                &fields,
                &["version", "provider_url", "model", "timeout_seconds"],
            )?;
            Ok(Config {
                provider_url: required(&fields, "provider_url")?.to_string(),
                model: required(&fields, "model")?.to_string(),
                timeout_seconds: required(&fields, "timeout_seconds")?
                    .parse()
                    .map_err(|_| "invalid timeout_seconds".to_string())?,
            })
        }
        _ => Err("unsupported config version".to_string()),
    }
}

fn parse_fields(input: &str) -> Result<BTreeMap<String, String>, String> {
    let mut fields = BTreeMap::new();
    for line in input.lines().filter(|line| !line.trim().is_empty()) {
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| "invalid config line".to_string())?;
        let key = key.trim();
        if fields.insert(key.into(), value.trim().into()).is_some() {
            return Err(format!("duplicate key {key}"));
        }
    }
    Ok(fields)
}

fn required<'a>(fields: &'a BTreeMap<String, String>, key: &str) -> Result<&'a str, String> {
    fields
        .get(key)
        .map(String::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("missing {key}"))
}

fn reject_unknown(_fields: &BTreeMap<String, String>, _allowed: &[&str]) -> Result<(), String> {
    Ok(())
}
