//! Pattern Converter Module
//!
//! Converts TagScout annotations to Log Scout LSP patterns.
//! Handles severity mapping, pattern validation, and metadata transformation.

use crate::pattern_engine::{Pattern, PatternMode, Severity};
use crate::tagscout::client::TagScoutAnnotation;
use thiserror::Error;

/// Conversion errors
#[derive(Error, Debug)]
pub enum ConversionError {
    #[error("Invalid pattern syntax: {0}")]
    InvalidPattern(String),

    #[error("Invalid severity level: {0}")]
    InvalidSeverity(String),

    #[error("Missing required field: {0}")]
    MissingField(String),

    #[error("Conversion error: {0}")]
    ConversionFailed(String),
}

/// Pattern converter configuration
#[derive(Debug, Clone)]
pub struct ConverterConfig {
    /// Convert multi-line patterns (default: true)
    pub convert_multiline: bool,

    /// Default context window for multi-line patterns
    pub default_context_window: usize,

    /// Validate regex patterns before conversion
    pub validate_regex: bool,

    /// Include inactive patterns (default: false)
    pub include_inactive: bool,

    /// Severity mapping overrides
    pub severity_mapping: Option<std::collections::HashMap<String, Severity>>,

    /// Product to service name mapping
    pub product_service_mapping: Option<std::collections::HashMap<String, String>>,
}

impl Default for ConverterConfig {
    fn default() -> Self {
        Self {
            convert_multiline: true,
            default_context_window: 10,
            validate_regex: true,
            include_inactive: false,
            severity_mapping: None,
            product_service_mapping: None,
        }
    }
}

/// Pattern converter
pub struct PatternConverter {
    config: ConverterConfig,
}

impl PatternConverter {
    /// Create a new pattern converter with default configuration
    pub fn new() -> Self {
        Self {
            config: ConverterConfig::default(),
        }
    }

    /// Create a new pattern converter with custom configuration
    pub fn with_config(config: ConverterConfig) -> Self {
        Self { config }
    }

    /// Extract named capture group names from a regex pattern
    /// Finds all (?P<name>...) groups and returns their names
    fn extract_capture_fields(pattern: &str) -> Vec<String> {
        use regex::Regex;

        // Match named groups: (?P<field_name>...)
        let named_group_regex = Regex::new(r"\(\?P<([^>]+)>").unwrap();

        named_group_regex
            .captures_iter(pattern)
            .filter_map(|cap| cap.get(1).map(|m| m.as_str().to_string()))
            .collect()
    }

    /// Convert a single TagScout annotation to an LSP pattern
    /// product: the product/service name from the collection (e.g., "jabber_prt", "webex_videomesh")
    pub fn convert(
        &self,
        annotation: &TagScoutAnnotation,
        product: Option<&str>,
    ) -> Result<Pattern, ConversionError> {
        // Skip non-production patterns unless configured otherwise
        if !annotation.production && !self.config.include_inactive {
            return Err(ConversionError::ConversionFailed(
                "Pattern is not production-ready".to_string(),
            ));
        }

        // Skip content annotations (they're for content filtering, not pattern matching)
        if annotation.content {
            return Err(ConversionError::ConversionFailed(
                "Content annotation".to_string(),
            ));
        }

        // Get first regex pattern (annotations can have multiple patterns)
        let pattern = annotation
            .regexes
            .first()
            .ok_or_else(|| ConversionError::MissingField("regexes".to_string()))?
            .clone();

        // Validate pattern if configured
        if self.config.validate_regex {
            self.validate_pattern(&pattern)?;
        }

        // Generate unique ID
        let id = self.generate_id(annotation);

        // Convert severity
        let severity = self.convert_severity(&annotation.severity)?;

        // Determine pattern mode
        let mode = self.determine_pattern_mode(&pattern);

        // Build name from raw_data or template
        let name = self.build_name(annotation);

        // Build annotation from template (TagScout template field becomes annotation message)
        let annotation_text = if !annotation.template.is_empty() {
            annotation.template.clone()
        } else if !annotation.raw_data.is_empty() {
            format!(
                "Pattern matching: {}",
                annotation.raw_data.chars().take(100).collect::<String>()
            )
        } else {
            "TagScout pattern".to_string()
        };

        // Get category (use first category if available)
        let category = annotation.category.first().cloned().unwrap_or_default();

        // Build tags from categories
        let tags = annotation.category.clone();

        // Build action from documentation
        let action = if !annotation.documentation.is_empty() {
            Some(annotation.documentation.clone())
        } else {
            None
        };

        // Extract capture field names from the regex pattern
        let capture_fields = Self::extract_capture_fields(&pattern);

        // Convert TagScout parameters to parameter extractors
        let parameter_extractors: Vec<crate::pattern_engine::ParameterExtractor> = annotation
            .parameters
            .iter()
            .map(|p| crate::pattern_engine::ParameterExtractor {
                name: p.name.clone(),
                regex: p.regex.clone(),
            })
            .collect();

        // Serialize original annotation as metadata
        let tagscout_metadata = serde_json::to_value(annotation).ok();

        Ok(Pattern {
            id,
            name,
            annotation: annotation_text,
            pattern,
            mode,
            severity,
            category,
            service: product.map(|s| s.to_string()),
            tags,
            action,
            expected_frequency: None,
            enabled: annotation.production,
            log_level_triggers: std::collections::HashMap::new(),
            condition_triggers: Vec::new(),
            capture_fields,
            parameter_extractors,
            tagscout_metadata,
        })
    }

    /// Convert multiple annotations with their product names
    pub fn convert_batch_with_products(
        &self,
        annotations: Vec<(String, TagScoutAnnotation)>,
    ) -> Result<Vec<Pattern>, ConversionError> {
        let mut patterns = Vec::new();
        let mut errors = Vec::new();

        for (product, annotation) in annotations {
            match self.convert(&annotation, Some(&product)) {
                Ok(pattern) => patterns.push(pattern),
                Err(e) => {
                    errors.push(format!("Product {}: {}", product, e));
                    continue;
                }
            }
        }

        if !errors.is_empty() {
            tracing::warn!(
                "Conversion errors: {} failed, {} succeeded",
                errors.len(),
                patterns.len()
            );
        }

        Ok(patterns)
    }

    /// Convert multiple annotations (legacy method, uses None for product)
    pub fn convert_batch(
        &self,
        annotations: Vec<TagScoutAnnotation>,
    ) -> Result<Vec<Pattern>, ConversionError> {
        let mut patterns = Vec::new();
        let mut errors = Vec::new();

        for annotation in annotations {
            match self.convert(&annotation, None) {
                Ok(pattern) => patterns.push(pattern),
                Err(e) => {
                    tracing::warn!(
                        "Failed to convert pattern '{}': {}",
                        annotation.id.to_hex(),
                        e
                    );
                    errors.push(e);
                }
            }
        }

        if patterns.is_empty() && !errors.is_empty() {
            return Err(ConversionError::ConversionFailed(format!(
                "Failed to convert any patterns: {} errors",
                errors.len()
            )));
        }

        tracing::info!(
            "Converted {} patterns ({} errors)",
            patterns.len(),
            errors.len()
        );

        Ok(patterns)
    }

    /// Generate a unique ID for the pattern
    fn generate_id(&self, annotation: &TagScoutAnnotation) -> String {
        // Use MongoDB ObjectId as unique identifier
        annotation.id.to_hex()
    }

    /// Build a name from annotation data
    fn build_name(&self, annotation: &TagScoutAnnotation) -> String {
        // Try to extract a meaningful name from the raw_data
        if !annotation.raw_data.is_empty() {
            // Take first meaningful part of the log line
            let parts: Vec<&str> = annotation.raw_data.split_whitespace().collect();
            if parts.len() > 3 {
                // Skip timestamp and log level, take the interesting part
                let name_part = parts[3..]
                    .iter()
                    .take(5)
                    .map(|s| *s)
                    .collect::<Vec<&str>>()
                    .join(" ");
                if name_part.len() > 50 {
                    format!("{}...", &name_part[..50])
                } else {
                    name_part
                }
            } else {
                annotation.raw_data.chars().take(50).collect::<String>()
            }
        } else if !annotation.template.is_empty() {
            // Use template as name
            annotation.template.chars().take(50).collect::<String>()
        } else if let Some(first_regex) = annotation.regexes.first() {
            // Use regex as fallback
            first_regex.chars().take(50).collect::<String>()
        } else {
            format!("Pattern {}", annotation.id.to_hex())
        }
    }

    /// Convert severity string to Severity enum
    fn convert_severity(&self, severity_str: &str) -> Result<Severity, ConversionError> {
        // Check custom mapping first
        if let Some(mapping) = &self.config.severity_mapping {
            if let Some(severity) = mapping.get(severity_str) {
                return Ok(*severity);
            }
        }

        // Default mapping
        match severity_str.to_lowercase().as_str() {
            "error" | "critical" | "fatal" | "severe" => Ok(Severity::Error),
            "warning" | "warn" | "caution" => Ok(Severity::Warning),
            "info" | "information" | "notice" => Ok(Severity::Info),
            "hint" | "debug" | "trace" | "verbose" => Ok(Severity::Hint),
            _ => {
                tracing::warn!("Unknown severity '{}', defaulting to Info", severity_str);
                Ok(Severity::Info)
            }
        }
    }

    /// Determine pattern mode based on pattern content
    fn determine_pattern_mode(&self, pattern: &str) -> PatternMode {
        if !self.config.convert_multiline {
            return PatternMode::SingleLine;
        }

        // Check for multi-line indicators
        if pattern.contains(r"\n") || pattern.contains("(?s)") || pattern.contains("(?m)") {
            PatternMode::MultiLine {
                context_lines: self.config.default_context_window,
            }
        } else {
            PatternMode::SingleLine
        }
    }

    /// Validate regex pattern
    fn validate_pattern(&self, pattern: &str) -> Result<(), ConversionError> {
        regex::Regex::new(pattern)
            .map_err(|e| ConversionError::InvalidPattern(format!("{}: {}", pattern, e)))?;
        Ok(())
    }

    /// Map product name to service name
    #[allow(dead_code)]
    fn map_product_to_service(&self, product: &str) -> Option<String> {
        if product.is_empty() {
            return None;
        }

        // Check custom mapping first
        if let Some(mapping) = &self.config.product_service_mapping {
            if let Some(service) = mapping.get(product) {
                return Some(service.clone());
            }
        }

        // Default: use product name as service name
        Some(product.to_lowercase())
    }

    /// Get converter configuration
    pub fn config(&self) -> &ConverterConfig {
        &self.config
    }
}

impl Default for PatternConverter {
    fn default() -> Self {
        Self::new()
    }
}

/// Batch conversion result
#[derive(Debug)]
pub struct ConversionResult {
    /// Successfully converted patterns
    pub patterns: Vec<Pattern>,

    /// Conversion errors
    pub errors: Vec<(String, ConversionError)>,

    /// Total annotations processed
    pub total: usize,

    /// Success rate
    pub success_rate: f32,
}

impl ConversionResult {
    /// Create a new conversion result
    pub fn new(
        patterns: Vec<Pattern>,
        errors: Vec<(String, ConversionError)>,
        total: usize,
    ) -> Self {
        let success_rate = if total > 0 {
            patterns.len() as f32 / total as f32
        } else {
            0.0
        };

        Self {
            patterns,
            errors,
            total,
            success_rate,
        }
    }

    /// Check if conversion was successful
    pub fn is_success(&self) -> bool {
        !self.patterns.is_empty()
    }

    /// Get summary string
    pub fn summary(&self) -> String {
        format!(
            "Converted {}/{} patterns ({:.1}% success rate, {} errors)",
            self.patterns.len(),
            self.total,
            self.success_rate * 100.0,
            self.errors.len()
        )
    }
}

/// Convert annotations with detailed result tracking
pub fn convert_with_result(
    annotations: Vec<TagScoutAnnotation>,
    config: Option<ConverterConfig>,
) -> ConversionResult {
    let converter = match config {
        Some(cfg) => PatternConverter::with_config(cfg),
        None => PatternConverter::new(),
    };

    let total = annotations.len();
    let mut patterns = Vec::new();
    let mut errors = Vec::new();

    for annotation in annotations {
        let id = annotation.id.to_hex();
        match converter.convert(&annotation, None) {
            Ok(pattern) => patterns.push(pattern),
            Err(e) => errors.push((id, e)),
        }
    }

    ConversionResult::new(patterns, errors, total)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_annotation() -> TagScoutAnnotation {
        TagScoutAnnotation {
            id: bson::oid::ObjectId::new(),
            name: "Test Error Pattern".to_string(),
            description: "Detects test errors".to_string(),
            pattern: r"ERROR:\s+(.+)".to_string(),
            severity: "error".to_string(),
            category: "errors".to_string(),
            product: "test-product".to_string(),
            component: "test-component".to_string(),
            tags: vec!["test".to_string()],
            action: "Check logs for details".to_string(),
            kb_id: "KB12345".to_string(),
            bug_id: "BUG67890".to_string(),
            version_introduced: "1.0.0".to_string(),
            version_fixed: "1.1.0".to_string(),
            active: true,
            last_updated: Some(bson::DateTime::now()),
            created_at: Some(bson::DateTime::now()),
            author: "test-author".to_string(),
            metadata: None,
        }
    }

    #[test]
    fn test_convert_annotation() {
        let converter = PatternConverter::new();
        let annotation = create_test_annotation();

        let result = converter.convert(&annotation);
        assert!(result.is_ok());

        let pattern = result.unwrap();
        assert_eq!(pattern.name, "Test Error Pattern");
        assert_eq!(pattern.severity, Severity::Error);
        assert_eq!(pattern.category, "errors");
    }

    #[test]
    fn test_severity_conversion() {
        let converter = PatternConverter::new();

        assert_eq!(
            converter.convert_severity("error").unwrap(),
            Severity::Error
        );
        assert_eq!(
            converter.convert_severity("warning").unwrap(),
            Severity::Warning
        );
        assert_eq!(converter.convert_severity("info").unwrap(), Severity::Info);
        assert_eq!(converter.convert_severity("hint").unwrap(), Severity::Hint);
    }

    #[test]
    fn test_pattern_mode_detection() {
        let converter = PatternConverter::new();

        // Single line pattern
        let single = converter.determine_pattern_mode(r"ERROR:\s+(.+)");
        assert_eq!(single, PatternMode::SingleLine);

        // Multi-line pattern
        let multi = converter.determine_pattern_mode(r"ERROR:\s+(.+)\n");
        assert!(matches!(multi, PatternMode::MultiLine { .. }));
    }

    #[test]
    fn test_validate_pattern() {
        let converter = PatternConverter::new();

        // Valid pattern
        assert!(converter.validate_pattern(r"ERROR:\s+(.+)").is_ok());

        // Invalid pattern
        assert!(converter.validate_pattern(r"ERROR:\s+((.+)").is_err());
    }

    #[test]
    fn test_build_action() {
        let converter = PatternConverter::new();
        let annotation = create_test_annotation();

        let action = converter.build_action(&annotation);
        assert!(action.is_some());

        let action_text = action.unwrap();
        assert!(action_text.contains("KB12345"));
        assert!(action_text.contains("BUG67890"));
        assert!(action_text.contains("1.1.0"));
    }

    #[test]
    fn test_build_tags() {
        let converter = PatternConverter::new();
        let annotation = create_test_annotation();

        let tags = converter.build_tags(&annotation);
        assert!(tags.contains(&"test".to_string()));
        assert!(tags.contains(&"test-product".to_string()));
        assert!(tags.contains(&"test-component".to_string()));
    }

    #[test]
    fn test_batch_conversion() {
        let converter = PatternConverter::new();
        let annotations = vec![
            create_test_annotation(),
            create_test_annotation(),
            create_test_annotation(),
        ];

        let result = converter.convert_batch(annotations);
        assert!(result.is_ok());

        let patterns = result.unwrap();
        assert_eq!(patterns.len(), 3);
    }

    #[test]
    fn test_inactive_pattern_filtering() {
        let converter = PatternConverter::new();
        let mut annotation = create_test_annotation();
        annotation.active = false;

        let result = converter.convert(&annotation);
        assert!(result.is_err());
    }

    #[test]
    fn test_conversion_result() {
        let annotations = vec![create_test_annotation(), create_test_annotation()];
        let result = convert_with_result(annotations, None);

        assert!(result.is_success());
        assert_eq!(result.total, 2);
        assert_eq!(result.patterns.len(), 2);
        assert!(result.success_rate > 0.0);
    }
}
