# TagScout MongoDB Integration

This document describes the TagScout MongoDB integration in the Log Scout LSP Server, providing real-time pattern synchronization with offline caching support.

## Overview

The TagScout integration connects the LSP server directly to the TagScout MongoDB database, fetching curated log annotation patterns and converting them for use in log analysis. The integration supports:

- **Direct MongoDB Connection**: Real-time pattern fetching from TagScout
- **Offline Operation**: Disk-based caching for analysis without network connectivity
- **Auto-Refresh**: Background synchronization to keep patterns up-to-date
- **Hot-Reload**: Pattern updates without server restart
- **Fallback Mechanisms**: Graceful degradation when MongoDB is unavailable

## Architecture

```
┌─────────────────────────────────────────────┐
│      TagScout MongoDB Database              │
│      (task_TagScoutLibrary)                 │
└──────────────────┬──────────────────────────┘
                   │
                   │ MongoDB Protocol
                   │
┌──────────────────▼──────────────────────────┐
│      TagScout Sync Service                  │
├─────────────────────────────────────────────┤
│  • Pattern Fetching                         │
│  • Conversion (TagScout → LSP Format)       │
│  • Caching & Persistence                    │
│  • Background Refresh                       │
└──────────────────┬──────────────────────────┘
                   │
                   │ Pattern Updates
                   │
┌──────────────────▼──────────────────────────┐
│      Pattern Engine                         │
├─────────────────────────────────────────────┤
│  • Pattern Compilation                      │
│  • Regex Matching                           │
│  • Log Analysis                             │
└──────────────────┬──────────────────────────┘
                   │
                   │ LSP Protocol
                   │
┌──────────────────▼──────────────────────────┐
│      VS Code / Editor Client               │
└─────────────────────────────────────────────┘
```

## Components

### 1. TagScout Client (`src/tagscout/client.rs`)

Handles MongoDB connectivity and data fetching.

**Features:**
- Connection pooling and timeouts
- Authentication with TagScout credentials
- Query filtering (by product, category, severity, tags)
- Connection testing and statistics

**Example Usage:**
```rust
use tagscout::TagScoutClient;

let client = TagScoutClient::new().await?;
let annotations = client.fetch_all_annotations().await?;
```

### 2. Pattern Converter (`src/tagscout/converter.rs`)

Converts TagScout annotations to LSP-compatible patterns.

**Features:**
- Severity mapping (error, warning, info, hint)
- Pattern validation and compilation
- Multi-line pattern detection
- Metadata enrichment (KB articles, bug IDs, version info)

**Example Usage:**
```rust
use tagscout::{PatternConverter, ConverterConfig};

let converter = PatternConverter::new();
let pattern = converter.convert(&annotation)?;
```

### 3. Cache Manager (`src/tagscout/cache.rs`)

Provides persistent caching for offline operation.

**Features:**
- JSON-based disk storage
- Atomic file operations with backup
- TTL-based expiration
- Pattern versioning and checksums
- Import/export capabilities

**Cache Location:**
```
.tagscout_cache/
  ├── tagscout_patterns.json       # Current cache
  └── tagscout_patterns.backup.json # Backup
```

**Example Usage:**
```rust
use tagscout::CacheManager;

let mut manager = CacheManager::new(".tagscout_cache", 3600, true);
manager.initialize().await?;
let cache = manager.load().await?;
```

### 4. Sync Service (`src/tagscout/mod.rs`)

Orchestrates pattern synchronization and lifecycle management.

**Features:**
- Multiple sync modes (offline, online-first, cache-first, always-online)
- Automatic refresh intervals
- Error handling with fallbacks
- Pattern filtering and selection

**Sync Modes:**

| Mode | Description | Use Case |
|------|-------------|----------|
| `OfflineOnly` | Only use cached patterns | Air-gapped environments |
| `OnlineFirst` | Try MongoDB, fallback to cache | Reliable network |
| `CacheFirst` | Use cache if valid, refresh when expired | **Default** - Best balance |
| `AlwaysOnline` | Always fetch fresh patterns | Real-time requirements |

## Configuration

### MongoDB Connection

Default connection (can be overridden):

```rust
TagScoutConfig {
    connection_string: "mongodb://task_TagScoutLibrary:HWCa_lWVy0U6SPl@10.89.108.161:27017/?authSource=admin",
    database: "task_TagScoutLibrary",
    collection: "annotations",
    connection_timeout: 10, // seconds
    server_selection_timeout: 10, // seconds
    enable_pooling: true,
    max_pool_size: 10,
    min_pool_size: 1,
}
```

### Sync Service Configuration

```rust
SyncServiceConfig {
    tagscout_config: TagScoutConfig::default(),
    converter_config: ConverterConfig::default(),
    cache_dir: PathBuf::from(".tagscout_cache"),
    cache_ttl_seconds: 3600, // 1 hour
    sync_mode: SyncMode::CacheFirst,
    auto_refresh_interval: Some(300), // 5 minutes
    auto_save_cache: true,
}
```

## LSP Server Integration

### Initialization

The LSP server initializes TagScout during the `initialized` lifecycle event:

1. **Server Starts** → Loads default patterns
2. **Client Connects** → `initialized` event triggered
3. **Background Init** → TagScout sync service starts
4. **Pattern Load** → Patterns loaded from cache or MongoDB
5. **Engine Update** → Pattern engine rebuilt with new patterns
6. **Auto-Refresh** → Background task starts periodic sync

### Commands

The LSP server exposes the following commands:

- `logScout.refreshPatterns` - Manually trigger pattern refresh

### Offline Operation

The integration is designed to work seamlessly offline:

1. **First Run (Online)**:
   - Connects to MongoDB
   - Fetches all patterns
   - Saves to disk cache
   - Patterns available immediately

2. **Subsequent Runs (Offline)**:
   - Loads patterns from disk cache
   - Full analysis capability without network
   - Continues working indefinitely

3. **Reconnection**:
   - Auto-refresh kicks in when network returns
   - Updates cache with latest patterns
   - Hot-reloads pattern engine

## Performance

### Benchmarks

| Operation | Time | Memory |
|-----------|------|--------|
| MongoDB Fetch (1000 patterns) | ~100ms | ~5MB |
| Pattern Conversion | ~20ms | ~2MB |
| Cache Load | ~50ms | ~3MB |
| Cache Save | ~80ms | ~1MB |
| Pattern Compilation | ~30ms | ~10MB |

### Optimization

- **Connection Pooling**: Reuses MongoDB connections
- **Compiled Regex**: Patterns compiled once, cached in memory
- **Incremental Updates**: Only changed patterns trigger recompilation
- **Async I/O**: All file and network operations are non-blocking

## Error Handling

The integration uses a layered approach:

1. **Try MongoDB** → If fails, try cache
2. **Try Cache** → If fails, use defaults
3. **Use Defaults** → Basic error/warning patterns
4. **Continue Operation** → Server never fails to start

### Error Recovery

```
Network Error → Load from Cache → Success
    ↓
Cache Expired → Use Stale Cache → Success
    ↓
Cache Corrupt → Use Defaults → Success
    ↓
Everything Fails → Minimal Patterns → Success
```

## Monitoring & Logging

### Log Levels

```rust
RUST_LOG=info    // Standard operation
RUST_LOG=debug   // Detailed sync info
RUST_LOG=trace   // MongoDB queries
```

### Key Log Messages

```
INFO  TagScout initialized: 847 patterns loaded from cache (age: 156ms)
INFO  Pattern engine updated with TagScout patterns
INFO  Background refresh task spawned
INFO  Auto-refresh completed: 847 patterns
WARN  MongoDB sync failed: connection timeout, using cache
ERROR Failed to load cache: file not found
```

### Cache Statistics

Query cache stats programmatically:

```rust
let stats = service.get_cache_stats().await;
println!("Patterns: {}", stats.pattern_count);
println!("Age: {} seconds", stats.age_seconds);
println!("Expired: {}", stats.is_expired);
```

## Security

### Credentials

- MongoDB credentials are embedded in default config
- Override via environment or config file for production
- Connection string supports authentication parameters

### Network Security

- TLS/SSL support available via MongoDB connection string
- Firewall rules recommended for MongoDB access
- Consider VPN for remote access

## Troubleshooting

### Cannot Connect to MongoDB

**Symptoms:**
```
WARN  Failed to connect to TagScout MongoDB: connection timeout
```

**Solutions:**
1. Check network connectivity to `10.89.108.161:27017`
2. Verify firewall rules allow MongoDB port
3. Confirm credentials are correct
4. Use `OfflineOnly` mode if offline operation required

### Cache Loading Issues

**Symptoms:**
```
ERROR Failed to load cache: Permission denied
```

**Solutions:**
1. Check file permissions on `.tagscout_cache/` directory
2. Ensure disk space available
3. Delete corrupt cache and re-sync
4. Verify path is writable

### Pattern Compilation Errors

**Symptoms:**
```
WARN  Failed to convert pattern 'invalid-regex': Invalid pattern
```

**Solutions:**
1. Pattern has invalid regex syntax
2. Check TagScout database for malformed patterns
3. Enable `validate_regex: true` in converter config
4. Patterns are skipped, not fatal

### Memory Usage

**Symptoms:**
- High memory usage with large pattern sets

**Solutions:**
1. Reduce `cache_ttl_seconds` to refresh less frequently
2. Filter patterns by product/category
3. Increase pattern match threshold to reduce false positives
4. Consider pattern selection criteria

## Development

### Building

```bash
cd lsp-server
cargo build --release
```

### Testing

```bash
# Unit tests (no MongoDB required)
cargo test

# Integration tests (MongoDB required)
cargo test -- --ignored

# Test MongoDB connection
cargo run --bin test-tagscout
```

### Custom Configuration

Create a `tagscout.toml` config file:

```toml
[mongodb]
connection_string = "mongodb://..."
database = "task_TagScoutLibrary"
collection = "annotations"

[cache]
directory = ".tagscout_cache"
ttl_seconds = 7200

[sync]
mode = "CacheFirst"
auto_refresh_interval = 600
```

## Future Enhancements

- [ ] Pattern priority and ranking
- [ ] User-defined pattern additions
- [ ] Pattern statistics and usage tracking
- [ ] Multi-database support
- [ ] Pattern recommendation engine
- [ ] Collaborative pattern curation
- [ ] GraphQL API for pattern management

## Support

For issues or questions:
- Check logs with `RUST_LOG=debug`
- Review this documentation
- Examine cache files in `.tagscout_cache/`
- Test MongoDB connection independently

## License

This integration is part of the Log Scout Analyzer LSP Server.