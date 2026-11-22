use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fs;
use std::path::{Path, PathBuf};

pub const DEFAULT_SUSPICIOUS_NAMES: &[&str] = &[
    "node",
    "python",
    "ruby",
    "java",
    "chrome",
    "chromium",
    "brave",
    "esbuild",
    "vite",
    "next-server",
    "webpack",
    "docker",
    "rustc",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FilterMode {
    #[default]
    Merge,
    Replace,
}

impl Serialize for FilterMode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            FilterMode::Merge => serializer.serialize_str("merge"),
            FilterMode::Replace => serializer.serialize_str("replace"),
        }
    }
}

impl<'de> Deserialize<'de> for FilterMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        match s.trim().to_lowercase().as_str() {
            "merge" => Ok(FilterMode::Merge),
            "replace" => Ok(FilterMode::Replace),
            other => Err(serde::de::Error::custom(format!(
                "invalid filter mode '{other}': expected 'merge' or 'replace'"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilterConfig {
    #[serde(default)]
    pub suspicious_names: Vec<String>,
    #[serde(default)]
    pub mode: FilterMode,
}

impl Default for FilterConfig {
    fn default() -> Self {
        Self {
            suspicious_names: Vec::new(),
            mode: FilterMode::Merge,
        }
    }
}

impl FilterConfig {
    /// Computes the effective, deduplicated, case-normalized list of suspicious process patterns.
    /// Empty and whitespace-only patterns are strictly filtered out to prevent false-positive matching.
    pub fn effective_names(&self) -> Vec<String> {
        let mut result = Vec::new();

        match self.mode {
            FilterMode::Merge => {
                // Add defaults first
                for &default_name in DEFAULT_SUSPICIOUS_NAMES {
                    let norm = default_name.trim().to_lowercase();
                    if !norm.is_empty() && !result.contains(&norm) {
                        result.push(norm);
                    }
                }
                // Add user patterns
                for user_name in &self.suspicious_names {
                    let norm = user_name.trim().to_lowercase();
                    if !norm.is_empty() && !result.contains(&norm) {
                        result.push(norm);
                    }
                }
            }
            FilterMode::Replace => {
                for user_name in &self.suspicious_names {
                    let norm = user_name.trim().to_lowercase();
                    if !norm.is_empty() && !result.contains(&norm) {
                        result.push(norm);
                    }
                }
            }
        }

        result
    }

    /// Alias for effective_names()
    pub fn resolved_suspicious_names(&self) -> Vec<String> {
        self.effective_names()
    }

    /// Checks if a given process name matches any of the active filter rules.
    #[allow(dead_code)]
    pub fn matches(&self, process_name: &str) -> bool {
        let proc_lower = process_name.to_lowercase();
        self.effective_names()
            .iter()
            .any(|filter| !filter.is_empty() && proc_lower.contains(filter))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub filters: FilterConfig,
}

#[derive(Debug, Clone)]
pub struct LoadResult {
    pub config: Config,
    pub source: Option<PathBuf>,
    pub warning: Option<String>,
}

#[allow(dead_code)]
impl Config {
    pub fn from_toml_str(content: &str) -> Result<Self, String> {
        toml::from_str::<Self>(content)
            .map_err(|e| format!("Failed to parse TOML configuration: {e}"))
    }

    pub fn load_from_path(path: &Path) -> Result<Self, String> {
        let content = fs::read_to_string(path)
            .map_err(|e| format!("Failed to read config file '{}': {e}", path.display()))?;
        Self::from_toml_str(&content)
            .map_err(|e| format!("In config file '{}': {e}", path.display()))
    }

    pub fn effective_names(&self) -> Vec<String> {
        self.filters.effective_names()
    }

    pub fn resolved_suspicious_names(&self) -> Vec<String> {
        self.filters.resolved_suspicious_names()
    }

    pub fn matches(&self, process_name: &str) -> bool {
        self.filters.matches(process_name)
    }

    /// Generates candidate config file paths using provided environment variables.
    /// Order:
    /// 1. `.opm.toml` in cwd
    /// 2. `$XDG_CONFIG_HOME/opm/config.toml` (if set and non-empty)
    /// 3. `$HOME/.config/opm/config.toml` (if set and non-empty)
    pub fn candidate_config_paths_from_env(
        xdg_config_home: &Option<std::ffi::OsString>,
        home: &Option<std::ffi::OsString>,
    ) -> Vec<PathBuf> {
        let mut candidates = Vec::new();
        candidates.push(PathBuf::from(".opm.toml"));

        if let Some(xdg) = xdg_config_home.as_ref().filter(|s| !s.is_empty()) {
            candidates.push(PathBuf::from(xdg).join("opm").join("config.toml"));
        }
        if let Some(h) = home.as_ref().filter(|s| !s.is_empty()) {
            candidates.push(PathBuf::from(h).join(".config").join("opm").join("config.toml"));
        }

        candidates
    }

    /// Generates candidate config file paths in order of precedence.
    pub fn candidate_config_paths() -> Vec<PathBuf> {
        Self::candidate_config_paths_from_env(
            &std::env::var_os("XDG_CONFIG_HOME"),
            &std::env::var_os("HOME"),
        )
    }

    /// Finds the first existing config file path among candidates, if any.
    pub fn find_config_path() -> Option<PathBuf> {
        Self::candidate_config_paths()
            .into_iter()
            .find(|path| path.is_file())
    }

    /// Loads config from candidates, returning defaults with warning if reading or parsing fails.
    pub fn load_from_candidates(candidates: &[PathBuf]) -> LoadResult {
        for path in candidates {
            if path.is_file() {
                match Self::load_from_path(path) {
                    Ok(config) => {
                        return LoadResult {
                            config,
                            source: Some(path.clone()),
                            warning: None,
                        };
                    }
                    Err(err_msg) => {
                        return LoadResult {
                            config: Config::default(),
                            source: Some(path.clone()),
                            warning: Some(err_msg),
                        };
                    }
                }
            }
        }

        LoadResult {
            config: Config::default(),
            source: None,
            warning: None,
        }
    }

    /// Loads configuration according to standard lookup rules.
    pub fn load() -> LoadResult {
        Self::load_from_candidates(&Self::candidate_config_paths())
    }

    /// Helper returning (Config, Option<warning>).
    pub fn load_with_warning() -> (Self, Option<String>) {
        let res = Self::load();
        (res.config, res.warning)
    }
}
