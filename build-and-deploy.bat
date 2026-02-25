@echo off
REM Build and deploy LSP server with auto-versioning

echo.
echo ========================================
echo   Building LSP Server
echo ========================================
echo.

REM Increment version
node increment-version.js
if %ERRORLEVEL% neq 0 (
    echo Failed to increment version
    exit /b 1
)

REM Build release
cargo build --release
if %ERRORLEVEL% neq 0 (
    echo Failed to build
    exit /b 1
)

REM Copy to extension directory
copy /Y target\release\log-scout-lsp-server.exe ..\vscode-extension\bin\log-scout-lsp-server-win.exe
if %ERRORLEVEL% neq 0 (
    echo Failed to copy binary
    exit /b 1
)

echo.
echo ========================================
echo   LSP Server Built and Deployed!
echo ========================================
echo.
