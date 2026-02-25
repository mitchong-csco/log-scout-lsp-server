#!/usr/bin/env pwsh
# Build and deploy LSP server with auto-versioning

Write-Host ""
Write-Host "========================================"
Write-Host "  Building LSP Server"
Write-Host "========================================"
Write-Host ""

# Increment version
node increment-version.js
if ($LASTEXITCODE -ne 0) {
    Write-Host "Failed to increment version" -ForegroundColor Red
    exit 1
}

# Build release
cargo build --release
if ($LASTEXITCODE -ne 0) {
    Write-Host "Failed to build" -ForegroundColor Red
    exit 1
}

# Copy to extension directory
Copy-Item -Path "target\release\log-scout-lsp-server.exe" -Destination "..\vscode-extension\bin\log-scout-lsp-server-win.exe" -Force
if ($LASTEXITCODE -ne 0) {
    Write-Host "Failed to copy binary" -ForegroundColor Red
    exit 1
}

Write-Host ""
Write-Host "========================================" -ForegroundColor Green
Write-Host "  LSP Server Built and Deployed!" -ForegroundColor Green
Write-Host "========================================" -ForegroundColor Green
Write-Host ""
