use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use crate::types::{SootieError, SootieResult};

const CONFIG_ENV: &str = "SOOTIE_CONFIG";
#[cfg(not(test))]
const CONFIG_FILE_NAME: &str = "sootie.config.toml";

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct SootieConfig {
    pub(crate) resolution: ResolutionConfig,
    pub(crate) vision: VisionSettings,
}

impl SootieConfig {
    pub(crate) fn load() -> Self {
        Self::load_from_default_path().unwrap_or_else(|error| {
            tracing::warn!(%error, "failed to load Sootie config; using defaults");
            Self::default()
        })
    }

    fn load_from_default_path() -> SootieResult<Self> {
        let Some(path) = config_path() else {
            return Ok(Self::default());
        };
        if !path.exists() {
            return Ok(Self::default());
        }
        Self::from_toml_str(&fs::read_to_string(path)?)
    }

    pub(crate) fn from_toml_str(input: &str) -> SootieResult<Self> {
        let table = parse_toml_like(input)?;
        let resolution = ResolutionConfig {
            strategy: string_setting(&table, "resolution", "strategy")
                .or_else(|| string_setting(&table, "perception", "strategy"))
                .or_else(|| string_setting(&table, "targeting", "strategy"))
                .or_else(|| string_setting(&table, "", "resolution_strategy"))
                .or_else(|| string_setting(&table, "", "target_strategy"))
                .map(|value| ResolutionStrategy::parse(&value))
                .transpose()?
                .unwrap_or_default(),
        };
        let vision = VisionSettings {
            enabled: bool_setting(&table, "vision", "enabled"),
            url: string_setting(&table, "vision", "url")
                .or_else(|| string_setting(&table, "vision", "base_url")),
            port: u16_setting(&table, "vision", "port")?,
            disabled: bool_setting(&table, "vision", "disabled"),
            connect_timeout: duration_setting_ms(&table, "vision", "connect_timeout_ms")?,
            ground_timeout: duration_setting_ms(&table, "vision", "timeout_ms")?
                .or(duration_setting_ms(&table, "vision", "ground_timeout_ms")?),
            confidence_threshold: f64_setting(&table, "vision", "confidence_threshold")?,
        };
        Ok(Self { resolution, vision })
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct ResolutionConfig {
    pub(crate) strategy: ResolutionStrategy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ResolutionStrategy {
    #[default]
    PlatformFirst,
    VisionOnly,
}

impl ResolutionStrategy {
    fn parse(value: &str) -> SootieResult<Self> {
        match normalized_setting(value).as_str() {
            "default"
            | "platform_first"
            | "platform_first_vision_fallback"
            | "desktop_first"
            | "ax_first"
            | "uia_first"
            | "at_spi_first"
            | "cdp_first"
            | "cascade" => Ok(Self::PlatformFirst),
            "vision" | "vision_only" | "vision_first" | "vision_first_only" => Ok(Self::VisionOnly),
            other => Err(SootieError::InvalidArguments(format!(
                "unsupported resolution.strategy '{other}'"
            ))),
        }
    }

    pub(crate) fn is_vision_only(self) -> bool {
        self == Self::VisionOnly
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct VisionSettings {
    pub(crate) enabled: Option<bool>,
    pub(crate) disabled: Option<bool>,
    pub(crate) url: Option<String>,
    pub(crate) port: Option<u16>,
    pub(crate) connect_timeout: Option<Duration>,
    pub(crate) ground_timeout: Option<Duration>,
    pub(crate) confidence_threshold: Option<f64>,
}

fn config_path() -> Option<PathBuf> {
    if let Ok(path) = env::var(CONFIG_ENV) {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            return Some(PathBuf::from(trimmed));
        }
    }
    #[cfg(test)]
    {
        None
    }
    #[cfg(not(test))]
    {
        dirs_next::home_dir().map(|home| home.join(".config").join(CONFIG_FILE_NAME))
    }
}

fn parse_toml_like(input: &str) -> SootieResult<BTreeMap<(String, String), String>> {
    let mut table = BTreeMap::new();
    let mut section = String::new();
    for (line_number, raw_line) in input.lines().enumerate() {
        let line = strip_comment(raw_line).trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = line
                .trim_start_matches('[')
                .trim_end_matches(']')
                .trim()
                .to_ascii_lowercase();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(SootieError::InvalidArguments(format!(
                "invalid config line {}",
                line_number + 1
            )));
        };
        table.insert(
            (section.clone(), key.trim().to_ascii_lowercase()),
            parse_scalar(value.trim()).map_err(|error| {
                SootieError::InvalidArguments(format!(
                    "invalid config line {}: {error}",
                    line_number + 1
                ))
            })?,
        );
    }
    Ok(table)
}

fn strip_comment(line: &str) -> &str {
    let mut in_string = false;
    let mut escaped = false;
    for (index, character) in line.char_indices() {
        match character {
            '\\' if in_string && !escaped => escaped = true,
            '"' if !escaped => in_string = !in_string,
            '#' if !in_string => return &line[..index],
            _ => escaped = false,
        }
    }
    line
}

fn parse_scalar(value: &str) -> Result<String, String> {
    if value.starts_with('"') {
        return parse_string(value);
    }
    Ok(value.trim().to_string())
}

fn parse_string(value: &str) -> Result<String, String> {
    if !value.ends_with('"') || value.len() < 2 {
        return Err("unterminated string".to_string());
    }
    let inner = &value[1..value.len() - 1];
    let mut output = String::new();
    let mut escaped = false;
    for character in inner.chars() {
        if escaped {
            output.push(match character {
                '"' => '"',
                '\\' => '\\',
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                other => other,
            });
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else {
            output.push(character);
        }
    }
    if escaped {
        return Err("dangling string escape".to_string());
    }
    Ok(output)
}

fn string_setting(
    table: &BTreeMap<(String, String), String>,
    section: &str,
    key: &str,
) -> Option<String> {
    table
        .get(&(section.to_ascii_lowercase(), key.to_ascii_lowercase()))
        .cloned()
}

fn bool_setting(
    table: &BTreeMap<(String, String), String>,
    section: &str,
    key: &str,
) -> Option<bool> {
    string_setting(table, section, key).and_then(|value| parse_bool(&value))
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Some(true),
        "false" | "0" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn u16_setting(
    table: &BTreeMap<(String, String), String>,
    section: &str,
    key: &str,
) -> SootieResult<Option<u16>> {
    string_setting(table, section, key)
        .map(|value| {
            value.parse::<u16>().map_err(|_| {
                SootieError::InvalidArguments(format!("{section}.{key} must be a valid port"))
            })
        })
        .transpose()
}

fn f64_setting(
    table: &BTreeMap<(String, String), String>,
    section: &str,
    key: &str,
) -> SootieResult<Option<f64>> {
    string_setting(table, section, key)
        .map(|value| {
            value.parse::<f64>().map_err(|_| {
                SootieError::InvalidArguments(format!("{section}.{key} must be a number"))
            })
        })
        .transpose()
}

fn duration_setting_ms(
    table: &BTreeMap<(String, String), String>,
    section: &str,
    key: &str,
) -> SootieResult<Option<Duration>> {
    string_setting(table, section, key)
        .map(|value| {
            value
                .parse::<u64>()
                .map(Duration::from_millis)
                .map_err(|_| {
                    SootieError::InvalidArguments(format!(
                        "{section}.{key} must be a non-negative millisecond integer"
                    ))
                })
        })
        .transpose()
}

fn normalized_setting(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .replace(['-', '.', ' '], "_")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_keeps_platform_first_resolution() {
        let config = SootieConfig::from_toml_str("").unwrap();

        assert_eq!(
            config.resolution.strategy,
            ResolutionStrategy::PlatformFirst
        );
        assert_eq!(config.vision, VisionSettings::default());
    }

    #[test]
    fn config_parses_vision_only_resolution_and_vision_settings() {
        let config = SootieConfig::from_toml_str(
            r#"
            [resolution]
            strategy = "vision-only"

            [vision]
            url = "http://127.0.0.1:9877"
            enabled = true
            confidence_threshold = 0.62
            connect_timeout_ms = 1500
            timeout_ms = 45000
            "#,
        )
        .unwrap();

        assert_eq!(config.resolution.strategy, ResolutionStrategy::VisionOnly);
        assert_eq!(config.vision.url.as_deref(), Some("http://127.0.0.1:9877"));
        assert_eq!(config.vision.enabled, Some(true));
        assert_eq!(config.vision.confidence_threshold, Some(0.62));
        assert_eq!(
            config.vision.connect_timeout,
            Some(Duration::from_millis(1500))
        );
        assert_eq!(
            config.vision.ground_timeout,
            Some(Duration::from_millis(45000))
        );
    }

    #[test]
    fn config_accepts_root_target_strategy_alias() {
        let config = SootieConfig::from_toml_str(r#"target_strategy = "vision""#).unwrap();

        assert_eq!(config.resolution.strategy, ResolutionStrategy::VisionOnly);
    }

    #[test]
    fn config_rejects_unknown_resolution_strategy() {
        let error = SootieConfig::from_toml_str(
            r#"
            [resolution]
            strategy = "something-else"
            "#,
        )
        .unwrap_err();

        assert!(error
            .to_string()
            .contains("unsupported resolution.strategy"));
    }
}
