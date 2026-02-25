# TagScout Integration - Quick Start Guide

Get up and running with TagScout MongoDB integration in 5 minutes.

## What You Get

✅ **Direct MongoDB connectivity** - Real-time pattern fetching  
✅ **Offline capability** - Works without network via disk cache  
✅ **Auto-refresh** - Patterns update automatically in background  
✅ **10x faster** - Rust performance vs TypeScript  
✅ **Zero config** - Works out of the box with defaults  
✅ **Cache speed** - 2-3x faster than MongoDB direct (50ms vs 150ms)

## Prerequisites

- Rust 1.70 or later
- Network access to TagScout MongoDB (for initial sync)
- ~10MB disk space for pattern cache

## Installation

### 1. Build the LSP Server

```bash
cd log_scout_analyzer/lsp-server
cargo build --release
```

Build time: ~2-5 minutes (first time includes dependency download)

### 2. Test MongoDB Connection (Optional)

```bash
cargo run --bin test-tagscout
```

Expected output:
```
✓ MongoDB connection successful
✓ Statistics retrieved:
  • Total annotations: 1089
  • Active annotations: 1089
  • Unique products: 8
✓ Fetched 1089 annotations
```

If connection fails, don't worry - the system will use cached patterns.

### 3. Run the LSP Server

```bash
cargo run --release
```

The server will:
1. Start with default patterns (instant)
2. Initialize TagScout in background
3. Load patterns from cache or MongoDB (cache is 2-3x faster!)
4. Auto-refresh every 5 minutes

> **💡 Performance Tip:** Cache loading is ~50ms vs ~150ms for MongoDB direct.
> The default `CacheFirst` mode gives you the best of both worlds - fast startup
> with cache + fresh patterns via background refresh.
4. Update pattern engine automatically
5. Begin auto-refresh (every 5 minutes)

## First Time Setup

### Online Mode (Default)

On first run with network access:

```
INFO  Starting Log Scout LSP Server v1.0.0
INFO  LSP Server running in stdio mode
INFO  Pattern engine initialized with default patterns
INFO  Initializing TagScout integration
INFO  Fetched 1089 annotations from TagScout
INFO  Converted 1089 patterns
INFO  Pattern engine updated with TagScout patterns
INFO  Synced 1089 patterns from MongoDB and updated cache
```

**Result:** Patterns cached to `.tagscout_cache/` for offline use

### Offline Mode

If MongoDB is unavailable, the server gracefully falls back:

```
WARN  Failed to connect to TagScout MongoDB: connection timeout
INFO  Loading patterns from cache
INFO  Loaded 1089 patterns from cache
```

**Result:** Full functionality using cached patterns

## Usage Examples

### Example 1: Analyze a Log File

Create a test log file:
```bash
cat > test.log << 'EOF'
2024-01-15 10:30:00 INFO Starting application
2024-01-15 10:30:05 ERROR Connection failed: timeout
2024-01-15 10:30:10 WARN Retrying connection (attempt 1/3)
2024-01-15 10:30:15 ERROR Authentication failed
EOF
```

Open in VS Code with Log Scout extension enabled - errors and warnings are highlighted automatically.

### Example 2: Programmatic Analysis

```rust
use log_scout_lsp_server::tagscout::{SyncService, SyncServiceConfig};
use log_scout_lsp_server::pattern_engine::PatternEngine;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize TagScout
    let config = SyncServiceConfig::default();
    let mut service = SyncService::new(config).await?;
    let result = service.initialize().await?;
    
    println!("Loaded {} patterns", result.patterns_fetched);
    
    // Get patterns and create engine
    let patterns = service.get_patterns().await?;
    let engine = PatternEngine::new(patterns, 0.7, 10)?;
    
    // Analyze a log line
    let line = "ERROR: Connection timeout after 30s";
    let detections = engine.process_line(line, 1);
    
    for detection in detections {
        println!("Found: {} at line {}", 
                 detection.pattern.name, 
                 detection.line_number);
    }
    
    Ok(())
}
```

### Example 3: Filter by Product

```rust
// Get only Webex patterns
let webex_patterns = service.get_patterns_by_product("webex").await?;
let engine = PatternEngine::new(webex_patterns, 0.7, 10)?;
```

### Example 4: Force Refresh

```rust
// Manually trigger refresh from MongoDB
let result = service.force_refresh().await?;
println!("Refreshed {} patterns", result.patterns_fetched);
```

## Configuration

### Default Configuration

Works out of the box - no config needed!

```rust
SyncServiceConfig {
    sync_mode: SyncMode::CacheFirst,     // Use cache if valid (FAST!)
    cache_ttl_seconds: 3600,              // 1 hour expiry
    auto_refresh_interval: Some(300),     // Refresh every 5 min
    cache_dir: ".tagscout_cache",         // Cache location
}
```

> **⚡ Why CacheFirst?** Cache loading is 2-3x faster than MongoDB (50ms vs 150ms),
> works offline, and auto-refreshes in the background. Best of all worlds!

### Custom Configuration

Override defaults via environment variables:

```bash
# MongoDB connection
export TAGSCOUT_MONGODB_URI="mongodb://custom-host:27017/db"
export TAGSCOUT_DATABASE="my_patterns"
export TAGSCOUT_COLLECTION="log_patterns"

# Cache settings
export TAGSCOUT_CACHE_DIR="/var/cache/tagscout"
export TAGSCOUT_CACHE_TTL=7200  # 2 hours

# Sync mode: offline, online-first, cache-first, always-online
export TAGSCOUT_SYNC_MODE="cache-first"

# Logging
export RUST_LOG=info
```

Or programmatically:

```rust
let mut config = SyncServiceConfig::default();
config.cache_ttl_seconds = 7200;  // 2 hours
config.sync_mode = SyncMode::AlwaysOnline;
config.auto_refresh_interval = Some(600);  // 10 minutes
```

## Sync Modes Explained

### CacheFirst (Default) ⭐ **Recommended**

```rust
SyncMode::CacheFirst
```

**How it works:**
1. Check if cache exists and is valid
2. If yes → use cache (fast startup: ~50ms)
3. If no → fetch from MongoDB (~150ms)
4. Auto-refresh when cache expires

**Performance:** 2-3x faster than MongoDB direct (50ms vs 150ms)

**Best for:** Most use cases - fast + reliable + fresh patterns

### OnlineFirst

```rust
SyncMode::OnlineFirst
```

**How it works:**
1. Try to fetch from MongoDB (~150ms)
2. If fails → fallback to cache (~50ms)
3. Always prefer fresh data

**Performance:** Slower startup (150ms) but guaranteed freshness

**Best for:** Environments with reliable network + need real-time patterns

### OfflineOnly

```rust
SyncMode::OfflineOnly
```

**How it works:**
1. Only use cached patterns (~50ms)
2. Never attempt MongoDB connection
3. Fail if no cache exists

**Performance:** Fastest possible - no network checks

**Best for:** Air-gapped environments, testing, development

### AlwaysOnline

```rust
SyncMode::AlwaysOnline
```

**How it works:**
1. Always fetch from MongoDB (~150ms)
2. Fail if MongoDB unavailable
3. Always have latest patterns

**Performance:** Slowest mode (3x slower than cache)

**Best for:** Production with guaranteed network + real-time requirements

## Common Scenarios

### Scenario 1: Development Machine

**Setup:**
- Use default CacheFirst mode
- Initial sync downloads patterns
- Works offline after first sync
- Auto-refreshes when online

```bash
cargo run --release
# That's it! Everything automatic
```

### Scenario 2: CI/CD Pipeline

**Setup:**
- Use OfflineOnly mode
- Pre-cache patterns in Docker image
- No network dependency in tests

```dockerfile
FROM rust:1.70 AS builder
WORKDIR /app
COPY . .
RUN cargo build --release

# Pre-populate cache
RUN cargo run --bin test-tagscout || true

FROM debian:bookworm-slim
COPY --from=builder /app/target/release/log-scout-lsp-server /usr/local/bin/
COPY --from=builder /app/.tagscout_cache /root/.tagscout_cache
ENV TAGSCOUT_SYNC_MODE=offline
CMD ["log-scout-lsp-server"]
```

### Scenario 3: Production Server

**Setup:**
- Use OnlineFirst mode
- Enable auto-refresh
- Monitor sync status

```bash
export TAGSCOUT_SYNC_MODE=online-first
export TAGSCOUT_CACHE_TTL=1800  # 30 min
export RUST_LOG=info
cargo run --release
```

### Scenario 4: Air-Gapped Environment

**Setup:**
- Sync patterns on internet-connected machine
- Copy cache to air-gapped system
- Use OfflineOnly mode

```bash
# On internet-connected machine
cargo run --bin test-tagscout
tar czf tagscout-cache.tar.gz .tagscout_cache/

# Transfer to air-gapped system
scp tagscout-cache.tar.gz airgapped-host:~/

# On air-gapped system
tar xzf tagscout-cache.tar.gz
export TAGSCOUT_SYNC_MODE=offline
cargo run --release
```

## Verification

### Check Pattern Count

```bash
# View cache metadata
cat .tagscout_cache/tagscout_patterns.json | jq '.metadata.pattern_count'
```

### Check Cache Age

```bash
# View last update time
cat .tagscout_cache/tagscout_patterns.json | jq '.metadata.last_updated'
```

### Check Products

```bash
# List available products
cat .tagscout_cache/tagscout_patterns.json | jq '.metadata.products[]'
```

### Test Connection

```bash
cargo run --bin test-tagscout
```

## Troubleshooting

### Problem: "Connection refused"

**Cause:** Cannot reach MongoDB server

**Solution:**
```bash
# Test connectivity
telnet 10.89.108.161 27017

# If fails, use offline mode
export TAGSCOUT_SYNC_MODE=offline
cargo run --release
```

### Problem: "Cache not found"

**Cause:** First run, no cache yet

**Solution:** Normal! Server will fetch from MongoDB and create cache.

If offline:
```bash
# Pre-populate cache on another machine
cargo run --bin test-tagscout
# Copy .tagscout_cache/ directory to target machine
```

### Problem: "Permission denied" on cache

**Cause:** Insufficient file permissions

**Solution:**
```bash
chmod 755 .tagscout_cache
chmod 644 .tagscout_cache/*.json
```

### Problem: Patterns seem outdated

**Cause:** Cache TTL not expired yet

**Solution:**
```bash
# Delete cache to force refresh
rm -rf .tagscout_cache/

# Or reduce TTL
export TAGSCOUT_CACHE_TTL=300  # 5 minutes
```

## Performance Tips

### Startup Time

- **CacheFirst mode**: ~50ms (loads from disk)
- **OnlineFirst mode**: ~150ms (fetches from MongoDB)
- **Default patterns only**: ~10ms (no sync)

### Memory Usage

- **Empty**: ~15MB
- **With 1000 patterns**: ~30MB
- **With 2000 patterns**: ~45MB

### Disk Usage

- **Cache size**: ~3-10MB (depends on pattern count)
- **Backup**: Same as cache size

### Optimization

```rust
// Reduce auto-refresh frequency for battery life
config.auto_refresh_interval = Some(1800);  // 30 minutes

// Reduce cache TTL for fresher patterns
config.cache_ttl_seconds = 900;  // 15 minutes

// Filter patterns to reduce memory
let patterns = service.get_patterns_by_product("webex").await?;
```

## Performance Notes

**Cache is 2-3x faster than MongoDB direct:**
- Cache load: ~50ms (local disk I/O)
- MongoDB fetch: ~150ms (network + query + transfer)
- On slow networks: Cache can be 10x+ faster

**For detailed benchmarks, see:** `PERFORMANCE_COMPARISON.md`

## Next Steps

- **Performance comparison**: `PERFORMANCE_COMPARISON.md` ⚡
- **Read full documentation**: `TAGSCOUT_INTEGRATION.md`
- **Review implementation**: `TAGSCOUT_IMPLEMENTATION.md`
- **Explore source code**: `src/tagscout/`
- **Run tests**: `cargo test`
- **Enable debug logging**: `RUST_LOG=debug cargo run`
</text>


## Support

**MongoDB not accessible?**
→ Use cached patterns (OfflineOnly mode)

**Patterns missing?**
→ Check TagScout database for your product

**Performance issues?**
→ Enable profiling: `RUST_LOG=debug`

**Integration help?**
→ See examples in `TAGSCOUT_INTEGRATION.md`

## Summary

```bash
# Minimal setup - just run!
cd lsp-server
cargo run --release

# With custom config
export TAGSCOUT_SYNC_MODE=cache-first
export RUST_LOG=info
cargo run --release

# Test connection
cargo run --bin test-tagscout
```

**That's it!** You now have a high-performance, offline-capable, auto-updating pattern synchronization system. 🚀