use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// Name of the kbmeta local state directory at workspace root.
pub const KB_DIR: &str = ".kb";
/// Name of the kbmeta configuration file inside `.kb`.
pub const CONFIG_FILE: &str = "config.toml";
/// Name of the SQLite index file inside `.kb`.
pub const DB_FILE: &str = "index.sqlite";

/// Runtime configuration loaded from `.kb/config.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Relative or absolute path to the Markdown documents root.
    #[serde(default = "default_documents_root")]
    pub documents_root: PathBuf,
    /// Optional preferred editor command for interactive metadata edits.
    #[serde(default)]
    pub editor: Option<String>,
    /// Preserve clean/context/suggestion intermediates under `.kb/intermediate`.
    #[serde(default)]
    pub keep_intermediate: bool,
    /// Cook command configuration.
    #[serde(default)]
    pub cook: CookConfig,
    /// Metadata provider runtime settings.
    #[serde(default)]
    pub provider: ProviderConfig,
}

/// Configuration options for the `cook` command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CookConfig {
    /// Prompt template name loaded from `.kb/prompts/<name>.md`.
    #[serde(default = "default_cook_prompt_name")]
    pub prompt_name: String,
    /// Maximum generated output filename stem length for cook output.
    #[serde(default = "default_cook_max_filename_len")]
    pub max_filename_len: usize,
    /// Output directory for kb-ready cook results.
    #[serde(default = "default_cook_output_dir")]
    pub output_dir: PathBuf,
}

impl Default for CookConfig {
    fn default() -> Self {
        Self {
            prompt_name: default_cook_prompt_name(),
            max_filename_len: default_cook_max_filename_len(),
            output_dir: default_cook_output_dir(),
        }
    }
}

/// Configuration for selecting and calling metadata providers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    /// Provider kind: `mock` or `nvidia`.
    pub kind: String,
    /// Endpoint URL for chat-completions style APIs.
    pub endpoint: String,
    /// Model name sent to the provider endpoint.
    pub model: String,
    /// Optional environment variable containing bearer token.
    #[serde(default)]
    pub api_key_env: Option<String>,
    /// Request timeout in seconds.
    pub timeout_seconds: u64,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            kind: "mock".to_string(),
            endpoint: "http://localhost:8000/v1/chat/completions".to_string(),
            model: "meta/llama-3.1-8b-instruct".to_string(),
            api_key_env: None,
            timeout_seconds: 60,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            documents_root: default_documents_root(),
            editor: None,
            keep_intermediate: false,
            cook: CookConfig::default(),
            provider: ProviderConfig::default(),
        }
    }
}

fn default_documents_root() -> PathBuf {
    PathBuf::from("knowledge/documents")
}

fn default_cook_prompt_name() -> String {
    "cook-default".to_string()
}

fn default_cook_max_filename_len() -> usize {
    32
}

fn default_cook_output_dir() -> PathBuf {
    PathBuf::from("knowledge/staging")
}

impl Config {
    /// Returns the `.kb` directory path for a workspace root.
    pub fn kb_dir(root: &Path) -> PathBuf {
        root.join(KB_DIR)
    }

    /// Returns the full path to `.kb/config.toml` for a workspace root.
    pub fn config_path(root: &Path) -> PathBuf {
        Self::kb_dir(root).join(CONFIG_FILE)
    }

    /// Returns the full path to `.kb/index.sqlite` for a workspace root.
    pub fn db_path(root: &Path) -> PathBuf {
        Self::kb_dir(root).join(DB_FILE)
    }

    /// Loads kbmeta configuration from disk.
    pub fn load(root: &Path) -> Result<Self> {
        Self::load_with(root, None)
    }

    /// Loads kbmeta configuration using precedence:
    /// `--config` path, then `KBMETA_CONFIG`, then `<root>/.kb/config.toml`.
    pub fn load_with(root: &Path, config_override: Option<&Path>) -> Result<Self> {
        let env_path = env::var("KBMETA_CONFIG").ok().map(PathBuf::from);
        let path = resolve_config_path(root, config_override, env_path.as_deref());

        let content = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let cfg = toml::from_str::<Self>(&content)
            .with_context(|| format!("invalid config file {}", path.display()))?;
        Ok(cfg)
    }

    /// Creates `.kb/config.toml` with defaults if it does not exist.
    pub fn write_default(root: &Path) -> Result<()> {
        Self::write_default_with(root, None)
    }

    /// Creates config file with precedence:
    /// `--config` path, then `KBMETA_CONFIG`, then `<root>/.kb/config.toml`.
    pub fn write_default_with(root: &Path, config_override: Option<&Path>) -> Result<()> {
        let env_path = env::var("KBMETA_CONFIG").ok().map(PathBuf::from);
        let path = resolve_config_path(root, config_override, env_path.as_deref());

        let parent = path
            .parent()
            .with_context(|| format!("{} has no parent directory", path.display()))?;
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;

        if path.exists() {
            return Ok(());
        }
        let content = toml::to_string_pretty(&Self::default())
            .with_context(|| "failed to serialize default config")?;
        fs::write(&path, content)
            .with_context(|| format!("failed to write {}", path.display()))?;
        Ok(())
    }
}

fn resolve_config_path(root: &Path, cli_path: Option<&Path>, env_path: Option<&Path>) -> PathBuf {
    if let Some(path) = cli_path {
        return resolve_relative_to_root(root, path);
    }
    if let Some(path) = env_path {
        return resolve_relative_to_root(root, path);
    }
    Config::config_path(root)
}

fn resolve_relative_to_root(root: &Path, path: &Path) -> PathBuf {
    crate::pathing::resolve_from_root(root, path)
}

#[cfg(test)]
mod tests {
    use super::resolve_config_path;
    use std::path::{Path, PathBuf};

    #[test]
    fn prefers_cli_path_over_env_and_default() {
        let root = Path::new("/tmp/work");
        let got = resolve_config_path(
            root,
            Some(Path::new("custom.toml")),
            Some(Path::new("env.toml")),
        );
        assert_eq!(got, PathBuf::from("/tmp/work/custom.toml"));
    }

    #[test]
    fn uses_env_path_when_cli_is_missing() {
        let root = Path::new("/tmp/work");
        let got = resolve_config_path(root, None, Some(Path::new("env.toml")));
        assert_eq!(got, PathBuf::from("/tmp/work/env.toml"));
    }

    #[test]
    fn falls_back_to_default_path() {
        let root = Path::new("/tmp/work");
        let got = resolve_config_path(root, None, None);
        assert_eq!(got, PathBuf::from("/tmp/work/.kb/config.toml"));
    }
}
