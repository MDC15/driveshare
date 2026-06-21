use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::warn;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_server")]
    pub server: ServerConfig,
    #[serde(default)]
    pub shares: Vec<ShareConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    #[serde(default)]
    pub tls_cert: Option<String>,
    #[serde(default)]
    pub tls_key: Option<String>,
}

fn default_server() -> ServerConfig {
    ServerConfig {
        host: "0.0.0.0".to_string(),
        port: 8080,
        tls_cert: None,
        tls_key: None,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareConfig {
    pub name: String,
    pub path: String,
    #[serde(default)]
    pub description: Option<String>,
}

impl Config {
    pub fn load(config_path: &Option<String>) -> anyhow::Result<Self> {
        if let Some(path) = config_path {
            let content = std::fs::read_to_string(path)?;
            return Ok(toml::from_str(&content)?);
        }

        let search_paths = vec![
            PathBuf::from("config.toml"),
            PathBuf::from("driveshare.toml"),
        ];

        for path in &search_paths {
            if path.exists() {
                let content = std::fs::read_to_string(path)?;
                return Ok(toml::from_str(&content)?);
            }
        }

        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                for name in &["config.toml", "driveshare.toml"] {
                    let path = exe_dir.join(name);
                    if path.exists() {
                        let content = std::fs::read_to_string(&path)?;
                        return Ok(toml::from_str(&content)?);
                    }
                }
            }
        }

        if let Some(config_dir) = directories::ProjectDirs::from("com", "driveshare", "driveshare")
        {
            let config_path = config_dir.config_dir().join("config.toml");
            if config_path.exists() {
                let content = std::fs::read_to_string(&config_path)?;
                return Ok(toml::from_str(&content)?);
            }
        }

        warn!("No configuration file found, using default configuration");
        Ok(Config::default())
    }

    pub fn share_by_name(&self, name: &str) -> Option<&ShareConfig> {
        self.shares.iter().find(|s| s.name == name)
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            server: default_server(),
            shares: vec![ShareConfig {
                name: "shared".to_string(),
                path: "./shared".to_string(),
                description: Some("Default shared folder".to_string()),
            }],
        }
    }
}
