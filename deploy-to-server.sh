#!/bin/bash
set -e

# Deploy log-scout-lsp-server to mitchong-podman
# This script deploys the LSP server container with Watchtower auto-update support

SERVER="mitchong-podman"
CONTAINER_NAME="log-scout-lsp-server"
IMAGE="ghcr.io/mitchong-csco/log-scout-lsp-server:latest"

echo "🚀 Deploying Log Scout LSP Server to $SERVER"
echo "================================================"

# Check if container already exists
echo "📋 Checking existing container..."
if ssh $SERVER "podman ps -a --format '{{.Names}}' | grep -q '^${CONTAINER_NAME}$'"; then
    echo "🛑 Stopping and removing existing container..."
    ssh $SERVER "podman stop $CONTAINER_NAME || true"
    ssh $SERVER "podman rm $CONTAINER_NAME || true"
fi

# Pull latest image
echo "📥 Pulling latest image..."
ssh $SERVER "podman pull $IMAGE"

# Create volumes if they don't exist
echo "📁 Creating volumes..."
ssh $SERVER "podman volume create log-scout-logs || true"
ssh $SERVER "podman volume create log-scout-cache || true"

# Deploy container (no port mapping - LSP runs in stdio mode, keep alive with sleep)
echo "🚢 Deploying container..."
ssh $SERVER "podman run -d \
    --name $CONTAINER_NAME \
    --restart unless-stopped \
    --label com.centurylinklabs.watchtower.enable=true \
    -v log-scout-logs:/tmp/log-scout-analyzer \
    -v log-scout-cache:/home/lspuser/.log-scout-analyzer \
    -e RUST_LOG=info \
    $IMAGE sleep infinity"

# Wait a moment for container to start
sleep 3

# Check status
echo ""
echo "📊 Container Status:"
ssh $SERVER "podman ps --filter name=$CONTAINER_NAME --format 'table {{.Names}}\t{{.Status}}\t{{.Image}}'"

echo ""
echo "✅ Deployment complete!"
echo ""
echo "💡 Useful commands:"
echo "  - Check logs:   ssh $SERVER 'podman logs -f $CONTAINER_NAME'"
echo "  - Check status: ssh $SERVER 'podman ps --filter name=$CONTAINER_NAME'"
echo "  - Stop:         ssh $SERVER 'podman stop $CONTAINER_NAME'"
echo "  - Restart:      ssh $SERVER 'podman restart $CONTAINER_NAME'"
echo ""
echo "🔄 Watchtower will automatically update this container when new versions are pushed"
