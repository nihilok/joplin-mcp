use std::path::{Path, PathBuf};

use clap::Parser;
use serde_json::Value;

use crate::error::{JoplinError, Result};

const DEFAULT_PORT: u16 = 41184;

/// Fast, standalone MCP server for Joplin's Data API.
#[derive(Debug, Clone, Parser)]
#[command(name = "joplin-mcp", version, about)]
pub struct Cli {
    /// Joplin Web Clipper API base URL (overrides --host and --port).
    #[arg(long, env = "JOPLIN_URL")]
    pub url: Option<String>,

    /// Joplin Web Clipper host.
    #[arg(long, env = "JOPLIN_HOST", default_value = "127.0.0.1")]
    pub host: String,

    /// Joplin Web Clipper port (overrides auto-detected api.port).
    #[arg(long, env = "JOPLIN_PORT")]
    pub port: Option<u16>,

    /// Joplin Web Clipper API token (auto-detected from Joplin settings if omitted).
    #[arg(long, env = "JOPLIN_TOKEN")]
    pub token: Option<String>,

    /// Request timeout in seconds.
    #[arg(long, env = "JOPLIN_TIMEOUT", default_value_t = 30)]
    pub timeout: u64,
}

/// A discovered local Joplin profile (Desktop or CLI).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileCandidate {
    pub label: &'static str,
    pub base_url: String,
    pub token: String,
}

/// Resolved runtime configuration for `JoplinClient`.
#[derive(Debug, Clone)]
pub struct Config {
    pub url_override: Option<String>,
    pub host: String,
    pub port_override: Option<u16>,
    pub base_url: String,
    pub token: Option<String>,
    pub timeout_secs: u64,
}

impl Config {
    /// Build configuration from CLI flags and environment variables.
    ///
    /// Also checks `JOPLIN_API_TOKEN` as a fallback if `JOPLIN_TOKEN` is unset.
    /// When no token is provided, `JoplinClient` auto-discovers Joplin Desktop
    /// first, then `joplin-cli`.
    ///
    /// # Errors
    ///
    /// Returns [`JoplinError::Config`] if `timeout` is zero.
    pub fn from_cli(cli: &Cli) -> Result<Self> {
        if cli.timeout == 0 {
            return Err(JoplinError::Config(
                "Timeout must be greater than 0 seconds".to_string(),
            ));
        }

        let url_override = cli
            .url
            .as_deref()
            .map(|u| u.trim_end_matches('/').to_string())
            .filter(|u| !u.is_empty());

        let base_url = match &url_override {
            Some(url) => url.clone(),
            None => format!("http://{}:{}", cli.host, cli.port.unwrap_or(DEFAULT_PORT)),
        };

        let token = cli
            .token
            .clone()
            .or_else(|| std::env::var("JOPLIN_API_TOKEN").ok())
            .filter(|t| !t.trim().is_empty());

        Ok(Self {
            url_override,
            host: cli.host.clone(),
            port_override: cli.port,
            base_url,
            token,
            timeout_secs: cli.timeout,
        })
    }

    /// Discover local Joplin profiles in priority order: Joplin Desktop first, then `joplin-cli`.
    #[must_use]
    pub fn discover_profiles(&self) -> Vec<ProfileCandidate> {
        let mut out = Vec::new();
        for (label, path) in candidate_settings_paths() {
            if let Some(candidate) = self.read_profile_file(label, &path) {
                out.push(candidate);
            }
        }
        out
    }

    fn read_profile_file(&self, label: &'static str, path: &Path) -> Option<ProfileCandidate> {
        let content = std::fs::read_to_string(path).ok()?;
        let json: Value = serde_json::from_str(&content).ok()?;
        parse_profile_json(
            label,
            &json,
            self.url_override.as_deref(),
            &self.host,
            self.port_override,
        )
    }
}

fn parse_profile_json(
    label: &'static str,
    json: &Value,
    url_override: Option<&str>,
    host: &str,
    port_override: Option<u16>,
) -> Option<ProfileCandidate> {
    let token = json
        .get("api.token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())?
        .to_string();

    let base_url = if let Some(url) = url_override {
        url.to_string()
    } else {
        let settings_port = json
            .get("api.port")
            .and_then(Value::as_u64)
            .and_then(|p| u16::try_from(p).ok());
        let port = port_override.or(settings_port).unwrap_or(DEFAULT_PORT);
        format!("http://{host}:{port}")
    };

    Some(ProfileCandidate {
        label,
        base_url,
        token,
    })
}

fn candidate_settings_paths() -> Vec<(&'static str, PathBuf)> {
    let mut paths = Vec::new();

    if let Some(home) = home_dir() {
        // Joplin Desktop first (faster sync via webclipper)
        paths.push((
            "joplin-desktop",
            home.join(".config/joplin-desktop/settings.json"),
        ));
        paths.push((
            "joplin-desktop",
            home.join("Library/Application Support/joplin-desktop/settings.json"),
        ));
        if let Some(appdata) = std::env::var_os("APPDATA") {
            paths.push((
                "joplin-desktop",
                PathBuf::from(appdata).join("Joplin/settings.json"),
            ));
        }

        // Joplin CLI fallback
        paths.push(("joplin-cli", home.join(".config/joplin/settings.json")));
    }

    paths
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_builds_default_base_url() {
        let cli = Cli {
            url: None,
            host: "127.0.0.1".to_string(),
            port: None,
            token: Some("secret".to_string()),
            timeout: 30,
        };
        let cfg = Config::from_cli(&cli);
        assert!(cfg.is_ok());
        if let Ok(cfg) = cfg {
            assert_eq!(cfg.base_url, "http://127.0.0.1:41184");
            assert_eq!(cfg.token.as_deref(), Some("secret"));
        }
    }

    #[test]
    fn config_trims_trailing_slash_on_url() {
        let cli = Cli {
            url: Some("http://localhost:41184/".to_string()),
            host: "127.0.0.1".to_string(),
            port: None,
            token: None,
            timeout: 15,
        };
        let cfg = Config::from_cli(&cli);
        assert!(cfg.is_ok());
        if let Ok(cfg) = cfg {
            assert_eq!(cfg.base_url, "http://localhost:41184");
        }
    }

    #[test]
    fn config_rejects_zero_timeout() {
        let cli = Cli {
            url: None,
            host: "127.0.0.1".to_string(),
            port: None,
            token: None,
            timeout: 0,
        };
        assert!(Config::from_cli(&cli).is_err());
    }

    #[test]
    fn parses_profile_json_with_custom_port() {
        let json = serde_json::json!({
            "api.token": "tok123",
            "api.port": 41185
        });
        let parsed = parse_profile_json("joplin-cli", &json, None, "127.0.0.1", None);
        assert_eq!(
            parsed,
            Some(ProfileCandidate {
                label: "joplin-cli",
                base_url: "http://127.0.0.1:41185".to_string(),
                token: "tok123".to_string(),
            })
        );
    }

    #[test]
    fn candidate_paths_put_desktop_before_cli() {
        let paths = candidate_settings_paths();
        let desktop_idx = paths.iter().position(|(l, _)| *l == "joplin-desktop");
        let cli_idx = paths.iter().position(|(l, _)| *l == "joplin-cli");
        assert!(desktop_idx.is_some());
        assert!(cli_idx.is_some());
        assert!(desktop_idx < cli_idx);
    }
}
