//! TagScout MongoDB Client
//!
//! Provides connectivity to the TagScout MongoDB database for fetching
//! curated log annotation patterns.

use mongodb::{
    bson::{doc, Document},
    options::ClientOptions,
    Client, Collection,
};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;
use tokio_stream::StreamExt;

/// TagScout client errors
#[derive(Error, Debug)]
pub enum TagScoutError {
    #[error("MongoDB connection error: {0}")]
    ConnectionError(#[from] mongodb::error::Error),

    #[error("Authentication failed: {0}")]
    AuthenticationError(String),

    #[error("Database not found: {0}")]
    DatabaseNotFound(String),

    #[error("Collection not found: {0}")]
    CollectionNotFound(String),

    #[error("Query error: {0}")]
    QueryError(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),
}

/// Raw annotation document from TagScout MongoDB
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagScoutAnnotation {
    #[serde(rename = "_id")]
    pub id: bson::oid::ObjectId,

    /// Raw example log line
    #[serde(default)]
    pub raw_data: String,

    /// Regular expression patterns (array - may have multiple variations)
    pub regexes: Vec<String>,

    /// Severity level (Info, Warning, Error, etc.)
    #[serde(default = "default_severity")]
    pub severity: String,

    /// Category tags (array)
    #[serde(default)]
    pub category: Vec<String>,

    /// Display template with parameter placeholders
    #[serde(default)]
    pub template: String,

    /// Production-ready flag
    #[serde(default)]
    pub production: bool,

    /// Whether this is a content annotation
    #[serde(default)]
    pub content: bool,

    /// Documentation notes
    #[serde(default)]
    pub documentation: String,

    /// Internal notes
    #[serde(default)]
    pub internal_notes: String,

    /// Multiline pattern flag
    #[serde(default)]
    pub multiline: Option<bool>,

    /// Whether this is an external annotation
    #[serde(default)]
    pub external: bool,

    /// BORG integration flag
    #[serde(default)]
    pub borg: bool,

    /// Parameter extraction regexes
    #[serde(default)]
    pub parameters: Vec<TagScoutParameter>,
}

/// Parameter definition for field extraction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagScoutParameter {
    /// Parameter name (matches template placeholder)
    pub name: String,

    /// Regex pattern to extract this parameter
    pub regex: String,

    /// Enum type (usually "None")
    #[serde(default)]
    pub r#enum: String,
}

fn default_severity() -> String {
    "info".to_string()
}

/// TagScout client configuration
#[derive(Debug, Clone)]
pub struct TagScoutConfig {
    /// MongoDB connection string
    pub connection_string: String,

    /// Database name
    pub database: String,

    /// Collection name
    pub collection: String,

    /// Connection timeout in seconds
    pub connection_timeout: u64,

    /// Server selection timeout in seconds
    pub server_selection_timeout: u64,

    /// Enable connection pooling
    pub enable_pooling: bool,

    /// Maximum pool size
    pub max_pool_size: u32,

    /// Minimum pool size
    pub min_pool_size: u32,
}

impl Default for TagScoutConfig {
    fn default() -> Self {
        // Get MongoDB connection from environment or use BDB production default
        let connection_string = std::env::var("TAGSCOUT_MONGODB_URI")
            .unwrap_or_else(|_| "mongodb://TagScoutLibrary_ro:4d6e2f2a60b17c87c2574fa3c1d39a18093a04d4@bdb-int-prod-mongos-1.cisco.com:27017,bdb-int-prod-mongos-2.cisco.com:27017/task_TagScoutLibrary?tls=true".to_string());
        
        let database = std::env::var("TAGSCOUT_DATABASE")
            .unwrap_or_else(|_| "task_TagScoutLibrary".to_string());
        
        Self {
            connection_string,
            database,
            collection: "jabber_prt_annotations".to_string(),
            connection_timeout: 10,
            server_selection_timeout: 10,
            enable_pooling: true,
            max_pool_size: 10,
            min_pool_size: 1,
        }
    }
}

/// Product configuration data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagScoutConfig_Data {
    #[serde(rename = "_id")]
    pub id: bson::oid::ObjectId,
    
    /// Valid categories for this product
    #[serde(default)]
    pub categories: Vec<String>,
    
    /// Valid severities for this product
    #[serde(default)]
    pub severities: Vec<String>,
}

/// Enum mapping data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagScoutEnum {
    #[serde(rename = "_id")]
    pub id: bson::oid::ObjectId,
    
    /// Enum name/identifier
    pub name: String,
    
    /// Production ready flag
    #[serde(default)]
    pub production: bool,
    
    /// Enum value mappings
    pub r#enum: std::collections::HashMap<String, String>,
}

/// TagScout MongoDB client
pub struct TagScoutClient {
    client: Client,
    database_name: String,
    config: TagScoutConfig,
}

impl TagScoutClient {
    /// Create a new TagScout client with default configuration
    pub async fn new() -> Result<Self, TagScoutError> {
        Self::with_config(TagScoutConfig::default()).await
    }

    /// Create a new TagScout client with custom configuration
    pub async fn with_config(config: TagScoutConfig) -> Result<Self, TagScoutError> {
        // Parse connection string
        let mut client_options = ClientOptions::parse(&config.connection_string).await?;

        // Configure timeouts
        client_options.connect_timeout = Some(Duration::from_secs(config.connection_timeout));
        client_options.server_selection_timeout =
            Some(Duration::from_secs(config.server_selection_timeout));

        // Configure connection pool
        if config.enable_pooling {
            client_options.max_pool_size = Some(config.max_pool_size);
            client_options.min_pool_size = Some(config.min_pool_size);
        }

        // Set application name for tracking
        client_options.app_name = Some("LogScout-LSP-Server".to_string());

        // Create client
        let client = Client::with_options(client_options)?;

        Ok(Self {
            client,
            database_name: config.database.clone(),
            config,
        })
    }

    /// Test the connection to MongoDB
    pub async fn test_connection(&self) -> Result<(), TagScoutError> {
        self.client
            .database("admin")
            .run_command(doc! { "ping": 1 }, None)
            .await
            .map_err(|e| TagScoutError::ConnectionError(e))?;

        tracing::info!("Successfully connected to TagScout MongoDB");
        Ok(())
    }

    /// Fetch all production annotations
    /// Get list of all products in the database
    pub async fn list_products(&self) -> Result<Vec<String>, TagScoutError> {
        let db = self.client.database(&self.database_name);
        let collections = db.list_collection_names(None).await?;
        
        let mut products = std::collections::HashSet::new();
        for collection in collections {
            // Extract product name from collection name (e.g., "jabber_prt_annotations" -> "jabber_prt")
            if let Some(pos) = collection.rfind('_') {
                let table_type = &collection[pos+1..];
                if table_type == "annotations" || table_type == "config" || table_type == "enums" {
                    let product = &collection[..pos];
                    products.insert(product.to_string());
                }
            }
        }
        
        Ok(products.into_iter().collect())
    }
    
    /// Fetch annotations from all products
    pub async fn fetch_all_annotations(&self) -> Result<Vec<TagScoutAnnotation>, TagScoutError> {
        let products = self.list_products().await?;
        tracing::info!("Found {} products in TagScout database", products.len());
        
        let mut all_annotations = Vec::new();
        
        for product in products {
            match self.fetch_product_annotations(&product).await {
                Ok(mut annotations) => {
                    tracing::info!("Fetched {} annotations from {}", annotations.len(), product);
                    all_annotations.append(&mut annotations);
                }
                Err(e) => {
                    tracing::warn!("Failed to fetch annotations from {}: {}", product, e);
                }
            }
        }
        
        tracing::info!("Total annotations fetched: {}", all_annotations.len());
        Ok(all_annotations)
    }
    
    /// Fetch annotations from a specific product
    pub async fn fetch_product_annotations(
        &self,
        product: &str,
    ) -> Result<Vec<TagScoutAnnotation>, TagScoutError> {
        let collection_name = format!("{}_annotations", product);
        let db = self.client.database(&self.database_name);
        let collection: Collection<TagScoutAnnotation> = db.collection(&collection_name);
        
        self.fetch_from_collection(&collection, doc! { "production": true }).await
    }

    /// Generic fetch from collection
    async fn fetch_from_collection<T>(
        &self,
        collection: &Collection<T>,
        filter: Document,
    ) -> Result<Vec<T>, TagScoutError>
    where
        T: for<'de> Deserialize<'de> + Unpin + Send + Sync,
    {
        let mut cursor = collection.find(filter, None).await?;
        let mut items = Vec::new();
        
        while let Some(result) = cursor.next().await {
            match result {
                Ok(item) => items.push(item),
                Err(e) => {
                    tracing::warn!("Failed to deserialize document: {}", e);
                    continue;
                }
            }
        }
        
        Ok(items)
    }
    
    /// Fetch config data from a specific product
    pub async fn fetch_product_config(
        &self,
        product: &str,
    ) -> Result<Option<TagScoutConfig_Data>, TagScoutError> {
        let collection_name = format!("{}_config", product);
        let db = self.client.database(&self.database_name);
        let collection: Collection<TagScoutConfig_Data> = db.collection(&collection_name);
        
        let mut items = self.fetch_from_collection(&collection, doc! {}).await?;
        Ok(items.pop())
    }
    
    /// Fetch enums from a specific product
    pub async fn fetch_product_enums(
        &self,
        product: &str,
    ) -> Result<Vec<TagScoutEnum>, TagScoutError> {
        let collection_name = format!("{}_enums", product);
        let db = self.client.database(&self.database_name);
        let collection: Collection<TagScoutEnum> = db.collection(&collection_name);
        
        self.fetch_from_collection(&collection, doc! { "production": true }).await
    }
    
    /// Fetch all config data from all products
    pub async fn fetch_all_configs(&self) -> Result<Vec<(String, TagScoutConfig_Data)>, TagScoutError> {
        let products = self.list_products().await?;
        let mut all_configs = Vec::new();
        
        for product in products {
            if let Ok(Some(config)) = self.fetch_product_config(&product).await {
                all_configs.push((product, config));
            }
        }
        
        Ok(all_configs)
    }
    
    /// Fetch all enums from all products
    pub async fn fetch_all_enums(&self) -> Result<Vec<(String, Vec<TagScoutEnum>)>, TagScoutError> {
        let products = self.list_products().await?;
        let mut all_enums = Vec::new();
        
        for product in products {
            if let Ok(enums) = self.fetch_product_enums(&product).await {
                if !enums.is_empty() {
                    all_enums.push((product, enums));
                }
            }
        }
        
        Ok(all_enums)
    }
