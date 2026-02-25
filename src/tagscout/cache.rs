//! Pattern Cache Module
//!
//! Provides persistent caching of TagScout patterns with:
//! - Disk-based storage for offline access
//! - Automatic expiration and refresh
//! - Fallback to cached patterns when MongoDB is unavailable
//! - Pattern versioning and updates

use crate::pattern_engine::Pattern;
use crate::tagscout::client::TagScoutAnnotation;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use thiserror::Error;
use tokio::fs;

/// Cache errors
#[derive(Error, Debug)]
pub enum CacheError {
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("Cache expired")]
    CacheExpired,

    #[error("Cache not found")]
    CacheNotFound,

    #[error("Invalid cache format: {0}")]
    InvalidFormat(String),
}

/// Cache metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheMetadata {
    /// Version of the cache format
    pub version: String,

    /// When the cache was created
    pub created_at: DateTime<Utc>,

    /// When the cache was last updated
    pub last_updated: DateTime<Utc>,

    /// Number of patterns in cache
    pub pattern_count: usize,

    /// Cache TTL in seconds
    pub ttl_seconds: u64,

    /// Source information
    pub source: CacheSource,

    /// Products included in cache
    pub products: Vec<String>,

    /// Categories included in cache
    pub categories: Vec<String>,
}

/// Cache source information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheSource {
    /// MongoDB connection string (sanitized)
    pub connection_info: String,

    /// Database name
    pub database: String,

    /// Collection name
    pub collection: String,
}

/// Cached pattern entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedPattern {
    /// Original TagScout annotation
    pub annotation: TagScoutAnnotation,

    /// Converted LSP pattern
    pub pattern: Pattern,

    /// When this pattern was cached
    pub cached_at: DateTime<Utc>,

    /// Pattern checksum for change detection
    pub checksum: String,
}

/// Pattern cache container
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternCache {
    /// Cache metadata
    pub metadata: CacheMetadata,

    /// Cached patterns by ID
    pub patterns: HashMap<String, CachedPattern>,
}

impl PatternCache {
    /// Create a new empty cache
    pub fn new(ttl_seconds: u64, source: CacheSource) -> Self {
        let now = Utc::now();
        Self {
            metadata: CacheMetadata {
                version: env!("CARGO_PKG_VERSION").to_string(),
                created_at: now,
                last_updated: now,
                pattern_count: 0,
                ttl_seconds,
                source,
                products: Vec::new(),
                categories: Vec::new(),
            },
            patterns: HashMap::new(),
        }
    }

    /// Normalize template placeholders to consistent format: {{ FIELD }}
    /// This ensures template substitution works correctly regardless of spacing inconsistencies
    fn normalize_template_fields(template: &str) -> String {
        use regex::Regex;

        // Match {{FIELD}}, {{ FIELD}}, {{FIELD }}, or {{ FIELD }}
        let re = Regex::new(r"\{\{\s*([A-Za-z_][A-Za-z0-9_]*)\s*\}\}").unwrap();

        // Replace all matches with standardized format: {{ FIELD }}
        re.replace_all(template, "{{ $1 }}").to_string()
    }

    /// Normalize parameter extractors to ensure consistency
    /// Trims whitespace from parameter names and their regex patterns
    ///
    /// NOTE: We do NOT normalize case (e.g., "code" vs "CODE") because:
    /// - TagScout data uses mixed conventions (UPPERCASE, camelCase, PascalCase, lowercase)
    /// - Parameter names must exactly match template placeholders (case-sensitive)
    /// - Normalizing case would require matching both params and templates perfectly
    /// - Original author's naming intent is preserved (e.g., "eventType" is more readable)
    /// - Case mismatches are rare and indicate data quality issues in MongoDB
    fn normalize_parameters(params: &mut [crate::pattern_engine::ParameterExtractor]) {
        for param in params.iter_mut() {
            // Trim whitespace from parameter names (must match template placeholders exactly)
            // Example: " request " -> "request", "CODE " -> "CODE"
            param.name = param.name.trim().to_string();

            // Trim leading/trailing whitespace from regex patterns
            param.regex = param.regex.trim().to_string();
        }
    }

    /// Add a pattern to the cache
    pub fn add_pattern(&mut self, annotation: TagScoutAnnotation, mut pattern: Pattern) {
        // Normalize template fields in annotation and category to ensure consistent substitution
        pattern.annotation = Self::normalize_template_fields(&pattern.annotation);
        pattern.category = Self::normalize_template_fields(&pattern.category);

        // Normalize parameter extractors (trim names and regex patterns)
        Self::normalize_parameters(&mut pattern.parameter_extractors);

        // Trim main regex pattern to remove leading/trailing whitespace
        pattern.pattern = pattern.pattern.trim().to_string();

        let checksum = Self::calculate_checksum(&annotation);
        let cached_pattern = CachedPattern {
            annotation,
            pattern,
            cached_at: Utc::now(),
            checksum,
        };

        self.patterns
            .insert(cached_pattern.pattern.id.clone(), cached_pattern);
        self.update_metadata();
    }

    /// Add multiple patterns to the cache
    pub fn add_patterns(&mut self, patterns: Vec<(TagScoutAnnotation, Pattern)>) {
        for (annotation, pattern) in patterns {
            self.add_pattern(annotation, pattern);
        }
    }

    /// Get a pattern by ID
    pub fn get_pattern(&self, id: &str) -> Option<&CachedPattern> {
        self.patterns.get(id)
    }

    /// Get all patterns
    pub fn get_all_patterns(&self) -> Vec<&Pattern> {
        self.patterns.values().map(|cp| &cp.pattern).collect()
    }

    /// Get patterns by category
    pub fn get_patterns_by_category(&self, category: &str) -> Vec<&Pattern> {
        self.patterns
            .values()
            .filter(|cp| cp.annotation.category.iter().any(|c| c == category))
            .map(|cp| &cp.pattern)
            .collect()
    }

    /// Check if cache is expired
    pub fn is_expired(&self) -> bool {
        let elapsed = Utc::now()
            .signed_duration_since(self.metadata.last_updated)
            .num_seconds();

        elapsed > self.metadata.ttl_seconds as i64
    }

    /// Get cache age in seconds
    pub fn age_seconds(&self) -> i64 {
        Utc::now()
            .signed_duration_since(self.metadata.last_updated)
            .num_seconds()
    }

    /// Update cache metadata
    fn update_metadata(&mut self) {
        self.metadata.last_updated = Utc::now();
        self.metadata.pattern_count = self.patterns.len();

        // Update products (use "jabber" as default since collection is jabber_prt_annotations)
        self.metadata.products = vec!["jabber".to_string()];

        // Update categories from all annotations
        let mut categories: Vec<String> = self
            .patterns
            .values()
            .flat_map(|cp| cp.annotation.category.iter())
            .map(|c| c.clone())
            .filter(|c| !c.is_empty())
            .collect();
        categories.sort();
        categories.dedup();
        self.metadata.categories = categories;
    }

    /// Calculate checksum for change detection
    fn calculate_checksum(annotation: &TagScoutAnnotation) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        annotation.regexes.hash(&mut hasher);
        annotation.severity.hash(&mut hasher);
        annotation.template.hash(&mut hasher);
        format!("{:x}", hasher.finish())
    }

    /// Merge with another cache (keeps newer patterns)
    pub fn merge(&mut self, other: PatternCache) {
        for (id, other_pattern) in other.patterns {
            match self.patterns.get(&id) {
                Some(existing) => {
                    // Keep the newer pattern
                    if other_pattern.cached_at > existing.cached_at {
                        self.patterns.insert(id, other_pattern);
                    }
                }
                None => {
                    self.patterns.insert(id, other_pattern);
                }
            }
        }
        self.update_metadata();
    }

    /// Clear all patterns
    pub fn clear(&mut self) {
        self.patterns.clear();
        self.update_metadata();
    }
}

/// Pattern cache manager with disk persistence
pub struct CacheManager {
    /// Cache directory path
    cache_dir: PathBuf,

    /// Current cache
    cache: Option<PatternCache>,

    /// Cache TTL in seconds
    ttl_seconds: u64,

    /// Auto-save on updates
    auto_save: bool,
}

impl CacheManager {
    /// Create a new cache manager
    pub fn new<P: AsRef<Path>>(cache_dir: P, ttl_seconds: u64, auto_save: bool) -> Self {
        Self {
            cache_dir: cache_dir.as_ref().to_path_buf(),
            cache: None,
            ttl_seconds,
            auto_save,
        }
    }

    /// Initialize cache directory
    pub async fn initialize(&self) -> Result<(), CacheError> {
        if !self.cache_dir.exists() {
            fs::create_dir_all(&self.cache_dir).await?;
            tracing::info!("Created cache directory: {:?}", self.cache_dir);
        }
        Ok(())
    }

    /// Get cache file path
    fn get_cache_path(&self) -> PathBuf {
        self.cache_dir.join("tagscout_patterns.json")
    }

    /// Get backup cache path
    fn get_backup_path(&self) -> PathBuf {
        self.cache_dir.join("tagscout_patterns.backup.json")
    }

    /// Load cache from disk
    pub async fn load(&mut self) -> Result<PatternCache, CacheError> {
        let cache_path = self.get_cache_path();

        if !cache_path.exists() {
            return Err(CacheError::CacheNotFound);
        }

        let content = fs::read_to_string(&cache_path).await?;
        let cache: PatternCache = serde_json::from_str(&content)?;

        tracing::info!(
            "Loaded cache with {} patterns (age: {}s)",
            cache.metadata.pattern_count,
            cache.age_seconds()
        );

        self.cache = Some(cache.clone());
        Ok(cache)
    }

    /// Save cache to disk
    pub async fn save(&self, cache: &PatternCache) -> Result<(), CacheError> {
        let cache_path = self.get_cache_path();
        let backup_path = self.get_backup_path();

        // Create backup of existing cache
        if cache_path.exists() {
            fs::copy(&cache_path, &backup_path).await?;
        }

        // Serialize cache
        let content = serde_json::to_string_pretty(cache)?;

        // Write to temp file first
        let temp_path = cache_path.with_extension("tmp");
        fs::write(&temp_path, content).await?;

        // Atomic rename
        fs::rename(&temp_path, &cache_path).await?;

        tracing::info!(
            "Saved cache with {} patterns to {:?}",
            cache.metadata.pattern_count,
            cache_path
        );

        Ok(())
    }

    /// Load cache or create new if not found
    pub async fn load_or_create(
        &mut self,
        source: CacheSource,
    ) -> Result<PatternCache, CacheError> {
        match self.load().await {
            Ok(cache) => Ok(cache),
            Err(CacheError::CacheNotFound) => {
                tracing::info!("No existing cache found, creating new cache");
                let cache = PatternCache::new(self.ttl_seconds, source);
                self.cache = Some(cache.clone());
                Ok(cache)
            }
            Err(e) => Err(e),
        }
    }

    /// Update cache with new patterns
    pub async fn update(
        &mut self,
        patterns: Vec<(TagScoutAnnotation, Pattern)>,
    ) -> Result<(), CacheError> {
        {
            let cache = self.cache.get_or_insert_with(|| {
                PatternCache::new(
                    self.ttl_seconds,
                    CacheSource {
                        connection_info: "unknown".to_string(),
                        database: "unknown".to_string(),
                        collection: "unknown".to_string(),
                    },
                )
            });

            cache.add_patterns(patterns);
        }

        if self.auto_save {
            if let Some(cache) = &self.cache {
                self.save(cache).await?;
            }
        }

        Ok(())
    }

    /// Get current cache
    pub fn get_cache(&self) -> Option<&PatternCache> {
        self.cache.as_ref()
    }

    /// Check if cache exists and is valid
    pub async fn is_cache_valid(&self) -> bool {
        // Check if cache file exists first
        let cache_path = self.get_cache_path();
        if !cache_path.exists() {
            return false;
        }

        // Try to read and check expiration
        match fs::read_to_string(&cache_path).await {
            Ok(content) => match serde_json::from_str::<PatternCache>(&content) {
                Ok(cache) => !cache.is_expired(),
                Err(_) => false,
            },
            Err(_) => false,
        }
    }

    /// Clear cache
    pub async fn clear(&mut self) -> Result<(), CacheError> {
        if let Some(cache) = &mut self.cache {
            cache.clear();
        }

        if self.auto_save {
            if let Some(cache) = &self.cache {
                self.save(cache).await?;
            }
        }

        Ok(())
    }

    /// Export cache to custom location
    pub async fn export<P: AsRef<Path>>(&self, path: P) -> Result<(), CacheError> {
        if let Some(cache) = &self.cache {
            let content = serde_json::to_string_pretty(cache)?;
            fs::write(path, content).await?;
            Ok(())
        } else {
            Err(CacheError::CacheNotFound)
        }
    }

    /// Import cache from custom location
    pub async fn import<P: AsRef<Path>>(&mut self, path: P) -> Result<(), CacheError> {
        let content = fs::read_to_string(path).await?;
        let cache: PatternCache = serde_json::from_str(&content)?;
        self.cache = Some(cache);
        Ok(())
    }

    /// Get cache statistics
    pub fn get_stats(&self) -> Option<CacheStats> {
        self.cache.as_ref().map(|cache| CacheStats {
            pattern_count: cache.metadata.pattern_count,
            age_seconds: cache.age_seconds(),
            is_expired: cache.is_expired(),
            ttl_seconds: cache.metadata.ttl_seconds,
            products: cache.metadata.products.len(),
            categories: cache.metadata.categories.len(),
            last_updated: cache.metadata.last_updated,
        })
    }
}

/// Cache statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheStats {
    pub pattern_count: usize,
    pub age_seconds: i64,
    pub is_expired: bool,
    pub ttl_seconds: u64,
    pub products: usize,
    pub categories: usize,
    pub last_updated: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pattern_engine::{PatternMode, Severity};

    fn create_test_annotation() -> TagScoutAnnotation {
        TagScoutAnnotation {
            id: bson::oid::ObjectId::new(),
            name: "Test Error".to_string(),
            description: "Test description".to_string(),
            pattern: r"ERROR:\s+(.+)".to_string(),
            severity: "error".to_string(),
            category: "errors".to_string(),
            product: "test-product".to_string(),
            component: "test-component".to_string(),
            tags: vec!["test".to_string()],
            action: "Check logs".to_string(),
            kb_id: "KB123".to_string(),
            bug_id: "BUG456".to_string(),
            version_introduced: "1.0".to_string(),
            version_fixed: "1.1".to_string(),
            active: true,
            last_updated: Some(bson::DateTime::now()),
            created_at: Some(bson::DateTime::now()),
            author: "test-author".to_string(),
            metadata: None,
        }
    }

    fn create_test_pattern() -> Pattern {
        Pattern {
            id: "test-error".to_string(),
            name: "Test Error".to_string(),
            annotation: "Test description".to_string(),
            pattern: r"ERROR:\s+(.+)".to_string(),
            mode: PatternMode::SingleLine,
            severity: Severity::Error,
            category: "errors".to_string(),
            service: Some("test-product".to_string()),
            tags: vec!["test".to_string()],
            action: Some("Check logs".to_string()),
            expected_frequency: None,
            enabled: true,
            log_level_triggers: std::collections::HashMap::new(),
            condition_triggers: Vec::new(),
            capture_fields: Vec::new(),
            parameter_extractors: Vec::new(),
            tagscout_metadata: None,
        }
    }

    #[test]
    fn test_cache_creation() {
        let source = CacheSource {
            connection_info: "test".to_string(),
            database: "test_db".to_string(),
            collection: "test_coll".to_string(),
        };

        let cache = PatternCache::new(3600, source);
        assert_eq!(cache.metadata.pattern_count, 0);
        assert!(!cache.is_expired());
    }

    #[test]
    fn test_add_pattern() {
        let source = CacheSource {
            connection_info: "test".to_string(),
            database: "test_db".to_string(),
            collection: "test_coll".to_string(),
        };

        let mut cache = PatternCache::new(3600, source);
        let annotation = create_test_annotation();
        let pattern = create_test_pattern();

        cache.add_pattern(annotation, pattern);
        assert_eq!(cache.metadata.pattern_count, 1);
        assert_eq!(cache.patterns.len(), 1);
    }

    #[test]
    fn test_get_pattern() {
        let source = CacheSource {
            connection_info: "test".to_string(),
            database: "test_db".to_string(),
            collection: "test_coll".to_string(),
        };

        let mut cache = PatternCache::new(3600, source);
        let annotation = create_test_annotation();
        let pattern = create_test_pattern();

        cache.add_pattern(annotation, pattern);

        let retrieved = cache.get_pattern("test-error");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().pattern.id, "test-error");
    }

    #[tokio::test]
    async fn test_cache_manager() {
        let temp_dir = std::env::temp_dir().join("tagscout_test_cache");
        let manager = CacheManager::new(&temp_dir, 3600, false);

        manager.initialize().await.unwrap();
        assert!(temp_dir.exists());

        // Cleanup
        let _ = tokio::fs::remove_dir_all(temp_dir).await;
    }
}
