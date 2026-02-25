#!/bin/bash
# Build and Test Script for TagScout Integration
# Usage: ./build-and-test.sh [--skip-mongodb] [--release]

set -e  # Exit on error

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Configuration
SKIP_MONGODB=false
RELEASE_MODE=false

# Parse arguments
for arg in "$@"; do
    case $arg in
        --skip-mongodb)
            SKIP_MONGODB=true
            shift
            ;;
        --release)
            RELEASE_MODE=true
            shift
            ;;
        --help)
            echo "Usage: ./build-and-test.sh [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  --skip-mongodb    Skip MongoDB connection tests"
            echo "  --release         Build in release mode (optimized)"
            echo "  --help           Show this help message"
            exit 0
            ;;
    esac
done

echo -e "${BLUE}╔════════════════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║  Log Scout LSP - TagScout Integration Builder     ║${NC}"
echo -e "${BLUE}╚════════════════════════════════════════════════════╝${NC}"
echo ""

# Step 1: Check Rust installation
echo -e "${YELLOW}[1/6] Checking Rust installation...${NC}"
if ! command -v cargo &> /dev/null; then
    echo -e "${RED}✗ Cargo not found. Please install Rust from https://rustup.rs/${NC}"
    exit 1
fi
RUST_VERSION=$(rustc --version)
echo -e "${GREEN}✓ Rust installed: $RUST_VERSION${NC}"
echo ""

# Step 2: Clean previous build
echo -e "${YELLOW}[2/6] Cleaning previous build...${NC}"
cargo clean
echo -e "${GREEN}✓ Build directory cleaned${NC}"
echo ""

# Step 3: Build the project
echo -e "${YELLOW}[3/6] Building LSP server...${NC}"
if [ "$RELEASE_MODE" = true ]; then
    echo -e "${BLUE}Building in RELEASE mode (optimized)...${NC}"
    cargo build --release
    BUILD_DIR="target/release"
else
    echo -e "${BLUE}Building in DEBUG mode...${NC}"
    cargo build
    BUILD_DIR="target/debug"
fi
echo -e "${GREEN}✓ Build completed successfully${NC}"
echo ""

# Step 4: Run unit tests
echo -e "${YELLOW}[4/6] Running unit tests...${NC}"
cargo test --lib
echo -e "${GREEN}✓ Unit tests passed${NC}"
echo ""

# Step 5: Test MongoDB connection (optional)
if [ "$SKIP_MONGODB" = false ]; then
    echo -e "${YELLOW}[5/6] Testing MongoDB connection...${NC}"
    echo -e "${BLUE}This will attempt to connect to TagScout MongoDB${NC}"

    if [ "$RELEASE_MODE" = true ]; then
        cargo run --release --bin test-tagscout || {
            echo -e "${YELLOW}⚠ MongoDB connection failed (this is OK for offline use)${NC}"
            echo -e "${BLUE}The system will use cached patterns when MongoDB is unavailable${NC}"
        }
    else
        cargo run --bin test-tagscout || {
            echo -e "${YELLOW}⚠ MongoDB connection failed (this is OK for offline use)${NC}"
            echo -e "${BLUE}The system will use cached patterns when MongoDB is unavailable${NC}"
        }
    fi
    echo ""
else
    echo -e "${YELLOW}[5/6] Skipping MongoDB connection test (--skip-mongodb flag)${NC}"
    echo ""
fi

# Step 6: Summary and next steps
echo -e "${YELLOW}[6/6] Build Summary${NC}"
echo -e "─────────────────────────────────────────────────────"
echo -e "${GREEN}✓ Build completed successfully${NC}"
echo -e "${GREEN}✓ Unit tests passed${NC}"

if [ -d ".tagscout_cache" ]; then
    CACHE_SIZE=$(du -sh .tagscout_cache | cut -f1)
    PATTERN_COUNT=$(cat .tagscout_cache/tagscout_patterns.json 2>/dev/null | grep -o '"pattern_count":[0-9]*' | grep -o '[0-9]*' || echo "0")
    echo -e "${GREEN}✓ Pattern cache exists: $CACHE_SIZE, $PATTERN_COUNT patterns${NC}"
else
    echo -e "${BLUE}ℹ No pattern cache found (will be created on first run)${NC}"
fi

echo ""
echo -e "${BLUE}╔════════════════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║              Build Successful! 🚀                   ║${NC}"
echo -e "${BLUE}╚════════════════════════════════════════════════════╝${NC}"
echo ""

# Display binary location and size
BINARY="$BUILD_DIR/log-scout-lsp-server"
if [ -f "$BINARY" ]; then
    BINARY_SIZE=$(du -sh "$BINARY" | cut -f1)
    echo -e "${GREEN}Binary location:${NC} $BINARY"
    echo -e "${GREEN}Binary size:${NC} $BINARY_SIZE"
    echo ""
fi

# Next steps
echo -e "${YELLOW}Next Steps:${NC}"
echo ""
echo -e "  ${GREEN}1.${NC} Run the LSP server:"
if [ "$RELEASE_MODE" = true ]; then
    echo -e "     ${BLUE}cargo run --release${NC}"
else
    echo -e "     ${BLUE}cargo run${NC}"
fi
echo ""
echo -e "  ${GREEN}2.${NC} Test MongoDB connection (if skipped):"
echo -e "     ${BLUE}cargo run --bin test-tagscout${NC}"
echo ""
echo -e "  ${GREEN}3.${NC} View cache status:"
echo -e "     ${BLUE}cat .tagscout_cache/tagscout_patterns.json | jq '.metadata'${NC}"
echo ""
echo -e "  ${GREEN}4.${NC} Configure sync mode:"
echo -e "     ${BLUE}export TAGSCOUT_SYNC_MODE=cache-first${NC}"
echo -e "     ${BLUE}export RUST_LOG=info${NC}"
echo ""
echo -e "  ${GREEN}5.${NC} Read documentation:"
echo -e "     ${BLUE}cat QUICK_START_TAGSCOUT.md${NC}"
echo -e "     ${BLUE}cat PERFORMANCE_COMPARISON.md${NC}"
echo ""

# Performance tips
echo -e "${YELLOW}Performance Tips:${NC}"
echo ""
echo -e "  • ${GREEN}Cache is 2-3x faster${NC} than MongoDB direct (50ms vs 150ms)"
echo -e "  • ${GREEN}CacheFirst mode${NC} (default) gives best balance"
echo -e "  • ${GREEN}OfflineOnly mode${NC} for development (fastest startup)"
echo -e "  • ${GREEN}Auto-refresh${NC} keeps patterns fresh in background"
echo ""

# Sync modes quick reference
echo -e "${YELLOW}Sync Modes Quick Reference:${NC}"
echo ""
echo -e "  ${BLUE}OfflineOnly${NC}    - Use cache only (fastest: ~50ms)"
echo -e "  ${BLUE}CacheFirst${NC}     - Use cache + auto-refresh (recommended)"
echo -e "  ${BLUE}OnlineFirst${NC}    - Try MongoDB, fallback to cache"
echo -e "  ${BLUE}AlwaysOnline${NC}   - Always fetch from MongoDB (slowest: ~150ms)"
echo ""

# Configuration example
echo -e "${YELLOW}Example Configuration:${NC}"
echo ""
echo -e "${BLUE}# Fast startup with auto-refresh (recommended)${NC}"
echo -e "export TAGSCOUT_SYNC_MODE=cache-first"
echo -e "export TAGSCOUT_CACHE_TTL=3600  # 1 hour"
echo -e "export RUST_LOG=info"
echo ""
echo -e "${BLUE}# Offline development mode (fastest)${NC}"
echo -e "export TAGSCOUT_SYNC_MODE=offline"
echo -e "export RUST_LOG=debug"
echo ""

echo -e "${GREEN}═══════════════════════════════════════════════════${NC}"
echo -e "${GREEN}Ready to analyze logs! 📊${NC}"
echo -e "${GREEN}═══════════════════════════════════════════════════${NC}"
echo ""

exit 0
