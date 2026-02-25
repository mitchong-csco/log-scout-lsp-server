//! TagScout Integration Module
//!
//! Provides MongoDB-based pattern synchronization from the TagScout library.
//! Supports offline operation with disk caching and automatic refresh.

pub mod cache;
pub mod client;
pub mod converter;

pub use cache::{CacheManager, CacheStats, PatternCache};
pub use client::{TagScoutAnnotation, TagScoutClient, TagScoutConfig, TagScoutError};
pub use converter::{ConversionError, ConverterConfig, PatternConverter};

use crate::pattern_engine::Pattern;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::RwLock;
use tokio::time::interval;

/// TagScout integration errors
#[derive(Error, Debug)]
pub enum IntegrationError {
    #[error("TagScout client error: {0}")]
    ClientError(#[from] TagScoutError),

    #[error("Cache error: {0}")]
    CacheError(#[from] cache::CacheError),

    #[error("Conversion error: {0}")]
    ConversionError(#[from] ConversionError),

    #[error("Sync error: {0}")]
    SyncError(String),

    #[error("Not initialized")]
    NotInitialized,
}

/// Sync mode configuration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncMode {
    /// Only use cached patterns, no network access
    OfflineOnly,

    /// Try network first, fallback to cache
    OnlineFirst,

    /// Try cache first, refresh in background
    CacheFirst,

    /// Always fetch fresh from network
    AlwaysOnline,
}

/// Sync result information
#[derive(Debug, Clone)]
pub struct SyncResult {
    /// Number of patterns fetched
    pub patterns_fetched: usize,

    /// Number of patterns cached
    pub patterns_cached: usize,

    /// Whether data came from cache
    pub from_cache: bool,

    /// Sync duration in milliseconds
    pub duration_ms: u64,

    /// Any warnings during sync
    pub warnings: Vec<String>,
}

/// TagScout sync service configuration
#[derive(Debug, Clone)]
pub struct SyncServiceConfig {
    /// TagScout client configuration
    pub tagscout_config: TagScoutConfig,

    /// Pattern converter configuration
    pub converter_config: ConverterConfig,

    /// Cache directory path
    pub cache_dir: PathBuf,

    /// Cache TTL in seconds
    pub cache_ttl_seconds: u64,

    /// Sync mode
    pub sync_mode: SyncMode,

    /// Auto-refresh interval in seconds (None = disabled)
    pub auto_refresh_interval: Option<u64>,

    /// Enable auto-save of cache
    pub auto_save_cache: bool,
}

impl Default for SyncServiceConfig {
    fn default() -> Self {
        let cache_dir = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(".tagscout_cache");

        Self {
            tagscout_config: TagScoutConfig::default(),
            converter_config: ConverterConfig::default(),
            cache_dir,
            cache_ttl_seconds: 3600, // 1 hour
            sync_mode: SyncMode::CacheFirst,
            auto_refresh_interval: Some(300), // 5 minutes
            auto_save_cache: true,
        }
    }
}

/// TagScout sync service
pub struct SyncService {
    config: SyncServiceConfig,
    client: Option<TagScoutClient>,
    cache_manager: Arc<RwLock<CacheManager>>,
    converter: PatternConverter,
    last_sync: Arc<RwLock<Option<chrono::DateTime<chrono::Utc>>>>,
}

impl SyncService {
    /// Create a new sync service
    pub async fn new(config: SyncServiceConfig) -> Result<Self, IntegrationError> {
        let cache_manager = CacheManager::new(
            &config.cache_dir,
            config.cache_ttl_seconds,
            config.auto_save_cache,
        );

        // Initialize cache directory
        cache_manager.initialize().await?;

        let converter = PatternConverter::with_config(config.converter_config.clone());

        Ok(Self {
            config,
            client: None,
            cache_manager: Arc::new(RwLock::new(cache_manager)),
            converter,
            last_sync: Arc::new(RwLock::new(None)),
        })
    }

    /// Initialize the service and perform initial sync
    pub async fn initialize(&mut self) -> Result<SyncResult, IntegrationError> {
        tracing::info!("Initializing TagScout sync service");

        // Attempt to connect to MongoDB based on sync mode
        if !matches!(self.config.sync_mode, SyncMode::OfflineOnly) {
            match TagScoutClient::with_config(self.config.tagscout_config.clone()).await {
                Ok(client) => {
                    tracing::info!("Connected to TagScout MongoDB");
                    self.client = Some(client);
                }
                Err(e) => {
                    tracing::warn!("Failed to connect to TagScout MongoDB: {}", e);
                    if matches!(
                        self.config.sync_mode,
                        SyncMode::AlwaysOnline | SyncMode::OnlineFirst
                    ) {
                        return Err(IntegrationError::ClientError(e));
                    }
                }
            }
        }

        // Perform initial sync
        self.sync().await
    }

    /// Sync patterns from TagScout or cache
    pub async fn sync(&self) -> Result<SyncResult, IntegrationError> {
        let start = std::time::Instant::now();
        let mut warnings = Vec::new();

        let result = match self.config.sync_mode {
            SyncMode::OfflineOnly => self.sync_from_cache().await?,
            SyncMode::AlwaysOnline => self.sync_from_mongodb().await?,
            SyncMode::OnlineFirst => match self.sync_from_mongodb().await {
                Ok(result) => result,
                Err(e) => {
                    warnings.push(format!("MongoDB sync failed: {}, using cache", e));
                    self.sync_from_cache().await?
                }
            },
            SyncMode::CacheFirst => {
                let cache_valid = {
                    let manager = self.cache_manager.read().await;
                    manager.is_cache_valid().await
                };

                if cache_valid {
                    self.sync_from_cache().await?
                } else {
                    match self.sync_from_mongodb().await {
                        Ok(result) => result,
                        Err(e) => {
                            warnings.push(format!("MongoDB sync failed: {}, using stale cache", e));
                            self.sync_from_cache().await?
                        }
                    }
                }
            }
        };

        // Update last sync time
        *self.last_sync.write().await = Some(chrono::Utc::now());

        let duration_ms = start.elapsed().as_millis() as u64;

        Ok(SyncResult {
            patterns_fetched: result.patterns_fetched,
            patterns_cached: result.patterns_cached,
            from_cache: result.from_cache,
            duration_ms,
            warnings,
        })
    }

    /// Sync patterns from MongoDB
    async fn sync_from_mongodb(&self) -> Result<SyncResult, IntegrationError> {
        let client = self
            .client
            .as_ref()
            .ok_or(IntegrationError::NotInitialized)?;

        tracing::info!("Fetching patterns from TagScout MongoDB");

        // Fetch all active annotations with product names
        let annotations_with_products = client.fetch_all_annotations().await?;
        let total_fetched = annotations_with_products.len();

        tracing::info!(
            "Fetched {} annotations from {} products, converting to patterns",
            total_fetched,
            annotations_with_products.iter().map(|(p, _)| p).collect::<std::collections::HashSet<_>>().len()
        );

        // Convert to patterns (preserving product information)
        let patterns = self.converter.convert_batch_with_products(annotations_with_products.clone())?;
        let patterns_count = patterns.len();

        // Update cache
        let mut cache_manager = self.cache_manager.write().await;
        let pattern_tuples: Vec<_> = annotations_with_products
            .into_iter()
            .zip(patterns.clone().into_iter())
            .map(|((_, annotation), pattern)| (annotation, pattern))
            .collect();

        cache_manager.update(pattern_tuples).await?;

        tracing::info!(
            "Synced {} patterns from MongoDB and updated cache",
            patterns_count
        );

        Ok(SyncResult {
            patterns_fetched: total_fetched,
            patterns_cached: patterns_count,
            from_cache: false,
            duration_ms: 0, // Will be set by caller
            warnings: Vec::new(),
        })
    }

    /// Sync patterns from cache
    async fn sync_from_cache(&self) -> Result<SyncResult, IntegrationError> {
        tracing::info!("Loading patterns from cache");

        let mut cache_manager = self.cache_manager.write().await;
        let source = cache::CacheSource {
            connection_info: "local-cache".to_string(),
            database: self.config.tagscout_config.database.clone(),
            collection: self.config.tagscout_config.collection.clone(),
        };

        let cache = cache_manager.load_or_create(source).await?;
        let patterns_count = cache.metadata.pattern_count;

        tracing::info!("Loaded {} patterns from cache", patterns_count);

        Ok(SyncResult {
            patterns_fetched: patterns_count,
            patterns_cached: patterns_count,
            from_cache: true,
            duration_ms: 0, // Will be set by caller
            warnings: Vec::new(),
        })
    }

    /// Get all patterns
    pub async fn get_patterns(&self) -> Result<Vec<Pattern>, IntegrationError> {
        let cache_manager = self.cache_manager.read().await;
        let cache = cache_manager
            .get_cache()
            .ok_or(IntegrationError::NotInitialized)?;

        Ok(cache.get_all_patterns().into_iter().cloned().collect())
    }

    /* Product-based filtering no longer supported -  collections are product-specific
    /// Get patterns by product
    pub async fn get_patterns_by_product(
        &self,
        product: &str,
    ) -> Result<Vec<Pattern>, IntegrationError> {
        let cache_manager = self.cache_manager.read().await;
        let cache = cache_manager
            .get_cache()
            .ok_or(IntegrationError::NotInitialized)?;

        Ok(cache
            .get_patterns_by_product(product)
            .into_iter()
            .cloned()
            .collect())
    }
    */

    /// Get patterns by category
    pub async fn get_patterns_by_category(
        &self,
        category: &str,
    ) -> Result<Vec<Pattern>, IntegrationError> {
        let cache_manager = self.cache_manager.read().await;
        let cache = cache_manager
            .get_cache()
            .ok_or(IntegrationError::NotInitialized)?;

        Ok(cache
            .get_patterns_by_category(category)
            .into_iter()
            .cloned()
            .collect())
    }

    /// Get cache statistics
    pub async fn get_cache_stats(&self) -> Option<CacheStats> {
        let cache_manager = self.cache_manager.read().await;
        cache_manager.get_stats()
    }

    /// Force refresh from MongoDB
    pub async fn force_refresh(&self) -> Result<SyncResult, IntegrationError> {
        tracing::info!("Force refreshing patterns from MongoDB");
        self.sync_from_mongodb().await
    }

    /// Start auto-refresh background task
    pub async fn start_auto_refresh(self: Arc<Self>) {
        if let Some(interval_secs) = self.config.auto_refresh_interval {
            tracing::info!("Starting auto-refresh every {} seconds", interval_secs);

            tokio::spawn(async move {
                let mut ticker = interval(Duration::from_secs(interval_secs));

                loop {
                    ticker.tick().await;

                    match self.sync().await {
                        Ok(result) => {
                            tracing::info!(
                                "Auto-refresh completed: {} patterns",
                                result.patterns_fetched
                            );
                        }
                        Err(e) => {
                            tracing::error!("Auto-refresh failed: {}", e);
                        }
                    }
                }
            });
        }
    }

    /// Get last sync time
    pub async fn last_sync_time(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        *self.last_sync.read().await
    }

    /// Check if patterns are available
    pub async fn has_patterns(&self) -> bool {
        let cache_manager = self.cache_manager.read().await;
        cache_manager
            .get_cache()
            .map_or(false, |c| c.metadata.pattern_count > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_sync_service_creation() {
        let config = SyncServiceConfig::default();
        let service = SyncService::new(config).await;
        assert!(service.is_ok());
    }

    #[tokio::test]
    async fn test_cache_first_mode() {
        let mut config = SyncServiceConfig::default();
        config.sync_mode = SyncMode::OfflineOnly;

        let service = SyncService::new(config).await;
        assert!(service.is_ok());
    }
}
