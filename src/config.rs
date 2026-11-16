use clap::Parser;

use crate::error::{JoplinError, Result};

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

    /// Joplin Web Clipper port.
    #[arg(long, env = "JOPLIN_PORT", default_value_t = 41184)]
    pub port: u16,

    /// Joplin Web Clipper API token.
    #[arg(long, env = "JOPLIN_TOKEN")]
    pub token: Option<String>,

    /// Request timeout in seconds.
    #[arg(long, env = "JOPLIN_TIMEOUT", default_value_t = 30)]
    pub timeout: u64,
}

/// Resolved runtime configuration for `JoplinClient`.
#[derive(Debug, Clone)]
pub struct Config {
    pub base_url: String,
    pub token: Option<String>,
    pub timeout_secs: u64,
}

impl Config {
    /// Build configuration from CLI flags and environment variables.
    ///
    /// Also checks `JOPLIN_API_TOKEN` as a fallback if `JOPLIN_TOKEN` is unset.
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

        let base_url = match &cli.url {
            Some(url) => url.trim_end_matches('/').to_string(),
            None => format!("http://{}:{}", cli.host, cli.port),
        };

        let token = cli
            .token
            .clone()
            .or_else(|| std::env::var("JOPLIN_API_TOKEN").ok())
            .filter(|t| !t.trim().is_empty());

        Ok(Self {
            base_url,
            token,
            timeout_secs: cli.timeout,
        })
    }

    /// Require a non-empty API token, returning a clear error if missing.
    ///
    /// # Errors
    ///
    /// Returns [`JoplinError::Config`] if no token was configured.
    pub fn require_token(&self) -> Result<&str> {
        self.token.as_deref().ok_or_else(|| {
            JoplinError::Config(
                "No Joplin API token configured. Set JOPLIN_TOKEN (or pass --token).".to_string(),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_builds_default_base_url() {
        let cli = Cli {
            url: None,
            host: "127.0.0.1".to_string(),
            port: 41184,
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
            port: 41184,
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
            port: 41184,
            token: None,
            timeout: 0,
        };
        assert!(Config::from_cli(&cli).is_err());
    }
}
