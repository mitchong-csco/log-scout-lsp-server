//! Configuration module for loading and managing pattern definitions
//!
//! This module handles loading patterns from YAML files and managing
//! extension settings.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

use crate::pattern_engine::{Pattern, PatternError};

/// Main configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Pattern definitions
    pub patterns: Vec<Pattern>,

    /// Plugin configurations
    #[serde(default)]
    pub plugins: PluginConfig,

    /// General settings
    #[serde(default)]
    pub settings: Settings,
}

/// Plugin configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginConfig {
    /// Jabber plugin configuration
    #[serde(default)]
    pub jabber: PluginSettings,

    /// Webex plugin configuration
    #[serde(default)]
    pub webex: PluginSettings,

    /// Custom plugin configuration
    #[serde(default)]
    pub custom: PluginSettings,
}

/// Individual plugin settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginSettings {
    /// Whether the plugin is enabled
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Path to plugin-specific pattern file
    pub config_path: Option<String>,

    /// Additional plugin-specific settings
    #[serde(default)]
    pub options: std::collections::HashMap<String, serde_yaml::Value>,
}

impl Default for PluginSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            config_path: None,
            options: std::collections::HashMap::new(),
        }
    }
}

fn default_true() -> bool {
    true
}

/// General extension settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Detection threshold (0.0 - 1.0)
    #[serde(default = "default_threshold")]
    pub detection_threshold: f32,

    /// Enable multi-line pattern matching
    #[serde(default = "default_true")]
    pub multiline_patterns: bool,

    /// Context window size for multi-line patterns
    #[serde(default = "default_context_window")]
    pub multiline_context_window: usize,

    /// Enable baseline learning
    #[serde(default = "default_true")]
    pub baseline_learning: bool,

    /// Enable cross-log correlation
    #[serde(default = "default_true")]
    pub correlation_enabled: bool,

    /// Maximum file size to process (in MB)
    #[serde(default = "default_max_file_size")]
    pub max_file_size_mb: usize,

    /// Streaming chunk size (in KB)
    #[serde(default = "default_chunk_size")]
    pub streaming_chunk_size_kb: usize,

    /// Enable background processing
    #[serde(default = "default_true")]
    pub background_processing: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            detection_threshold: default_threshold(),
            multiline_patterns: true,
            multiline_context_window: default_context_window(),
            baseline_learning: true,
            correlation_enabled: true,
            max_file_size_mb: default_max_file_size(),
            streaming_chunk_size_kb: default_chunk_size(),
            background_processing: true,
        }
    }
}

fn default_threshold() -> f32 {
    0.85
}

fn default_context_window() -> usize {
    10
}

fn default_max_file_size() -> usize {
    100
}

fn default_chunk_size() -> usize {
    512
}

/// Load configuration from a YAML file
pub fn load_config<P: AsRef<Path>>(path: P) -> Result<Config, PatternError> {
    let content = fs::read_to_string(path)
        .map_err(|e| PatternError::ConfigError(format!("Failed to read config file: {}", e)))?;

    parse_config(&content)
}

/// Parse configuration from YAML string
pub fn parse_config(yaml: &str) -> Result<Config, PatternError> {
    serde_yaml::from_str(yaml)
        .map_err(|e| PatternError::ConfigError(format!("Failed to parse YAML: {}", e)))
}

/// Load patterns from a YAML file
pub fn load_patterns<P: AsRef<Path>>(path: P) -> Result<Vec<Pattern>, PatternError> {
    let config = load_config(path)?;
    Ok(config.patterns)
}

/// Load patterns from YAML string
pub fn parse_patterns(yaml: &str) -> Result<Vec<Pattern>, PatternError> {
    #[derive(Deserialize)]
    struct PatternFile {
        patterns: Vec<Pattern>,
    }

    let pattern_file: PatternFile = serde_yaml::from_str(yaml)
        .map_err(|e| PatternError::ConfigError(format!("Failed to parse patterns: {}", e)))?;

    Ok(pattern_file.patterns)
}

/// Merge multiple pattern files into one configuration
pub fn merge_patterns(pattern_sets: Vec<Vec<Pattern>>) -> Vec<Pattern> {
    let mut merged = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();

    for patterns in pattern_sets {
        for pattern in patterns {
            // Only add if we haven't seen this ID before
            if seen_ids.insert(pattern.id.clone()) {
                merged.push(pattern);
            }
        }
    }

    merged
}

/// Validate a configuration
pub fn validate_config(config: &Config) -> Result<(), PatternError> {
    // Check for duplicate pattern IDs
    let mut seen_ids = std::collections::HashSet::new();
    for pattern in &config.patterns {
        if !seen_ids.insert(&pattern.id) {
            return Err(PatternError::ConfigError(format!(
                "Duplicate pattern ID: {}",
                pattern.id
            )));
        }
    }

    // Validate threshold range
    if config.settings.detection_threshold < 0.0 || config.settings.detection_threshold > 1.0 {
        return Err(PatternError::ConfigError(
            "Detection threshold must be between 0.0 and 1.0".to_string(),
        ));
    }

    // Validate context window
    if config.settings.multiline_context_window == 0 {
        return Err(PatternError::ConfigError(
            "Context window must be greater than 0".to_string(),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pattern_engine::{PatternMode, Severity};

    #[test]
    fn test_parse_simple_config() {
        let yaml = r#"
patterns:
  - id: "test-error"
    name: "Test Error"
    description: "A test error pattern"
    pattern: "ERROR: (.*)"
    severity: error
    category: "testing"
"#;

        let config = parse_config(yaml).unwrap();
        assert_eq!(config.patterns.len(), 1);
        assert_eq!(config.patterns[0].id, "test-error");
    }

    #[test]
    fn test_parse_patterns_only() {
        let yaml = r#"
patterns:
  - id: "error-1"
    name: "Error 1"
    description: "First error"
    pattern: "ERROR"
    severity: error
    category: "errors"
  - id: "warning-1"
    name: "Warning 1"
    description: "First warning"
    pattern: "WARN"
    severity: warning
    category: "warnings"
"#;

        let patterns = parse_patterns(yaml).unwrap();
        assert_eq!(patterns.len(), 2);
        assert_eq!(patterns[0].id, "error-1");
        assert_eq!(patterns[1].id, "warning-1");
    }

    #[test]
    fn test_default_settings() {
        let settings = Settings::default();
        assert_eq!(settings.detection_threshold, 0.85);
        assert_eq!(settings.multiline_context_window, 10);
        assert!(settings.multiline_patterns);
        assert!(settings.baseline_learning);
    }

    #[test]
    fn test_validate_config() {
        let config = Config {
            patterns: vec![],
            plugins: PluginConfig::default(),
            settings: Settings::default(),
        };

        assert!(validate_config(&config).is_ok());
    }

    #[test]
    fn test_validate_duplicate_ids() {
        let yaml = r#"
patterns:
  - id: "duplicate"
    name: "First"
    description: "First pattern"
    pattern: "TEST"
    severity: error
    category: "test"
  - id: "duplicate"
    name: "Second"
    description: "Second pattern"
    pattern: "TEST2"
    severity: error
    category: "test"
"#;

        let config = parse_config(yaml).unwrap();
        let result = validate_config(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_merge_patterns() {
        let set1 = vec![Pattern {
            id: "p1".to_string(),
            name: "Pattern 1".to_string(),
            annotation: "First".to_string(),
            pattern: "TEST1".to_string(),
            mode: PatternMode::SingleLine,
            severity: Severity::Error,
            category: "test".to_string(),
            service: None,
            tags: vec![],
            action: None,
            expected_frequency: None,
            enabled: true,
            log_level_triggers: std::collections::HashMap::new(),
            condition_triggers: Vec::new(),
            capture_fields: Vec::new(),
            parameter_extractors: Vec::new(),
            tagscout_metadata: None,
        }];

        let set2 = vec![Pattern {
            id: "p2".to_string(),
            name: "Pattern 2".to_string(),
            annotation: "Second".to_string(),
            pattern: "TEST2".to_string(),
            mode: PatternMode::SingleLine,
            severity: Severity::Warning,
            category: "test".to_string(),
            service: None,
            tags: vec![],
            action: None,
            expected_frequency: None,
            enabled: true,
            log_level_triggers: std::collections::HashMap::new(),
            condition_triggers: Vec::new(),
            capture_fields: Vec::new(),
            parameter_extractors: Vec::new(),
            tagscout_metadata: None,
        }];

        let merged = merge_patterns(vec![set1, set2]);
        assert_eq!(merged.len(), 2);
    }
}
