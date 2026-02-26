# Multi-stage build for Rust LSP server
FROM rust:latest as builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Set working directory
WORKDIR /app

# Copy all source
COPY . .

# Build the LSP server
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN useradd -r -s /bin/false lspuser

# Copy binary from builder stage
COPY --from=builder /app/target/release/log-scout-lsp-server /usr/local/bin/

# Set permissions
RUN chmod +x /usr/local/bin/log-scout-lsp-server
RUN chown lspuser:lspuser /usr/local/bin/log-scout-lsp-server

# Switch to non-root user
USER lspuser

# Expose LSP server port (if using TCP mode)
EXPOSE 8080

# Set entrypoint
ENTRYPOINT ["log-scout-lsp-server"]

# Default arguments (can be overridden)
CMD []
