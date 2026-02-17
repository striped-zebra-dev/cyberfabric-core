use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Configuration for the `file_parser` module
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileParserConfig {
    #[serde(default = "default_max_file_size_mb")]
    pub max_file_size_mb: u64,

    /// Base directory for local file parsing. When set, only files under this
    /// directory (after symlink resolution / canonicalization) are allowed.
    /// Recommended for production deployments to prevent path-traversal attacks.
    #[serde(default)]
    pub allowed_local_base_dir: Option<PathBuf>,
}

impl Default for FileParserConfig {
    fn default() -> Self {
        Self {
            max_file_size_mb: default_max_file_size_mb(),
            allowed_local_base_dir: None,
        }
    }
}

fn default_max_file_size_mb() -> u64 {
    100
}
