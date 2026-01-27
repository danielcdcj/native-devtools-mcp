#!/bin/bash
set -euo pipefail

# Build macOS app bundle for native-devtools-mcp
#
# Environment variables (required):
#   SIGN_IDENTITY - Code signing identity (e.g., "Developer ID Application: ...")
#
# Environment variables (optional):
#   TARGET        - Rust target (default: aarch64-apple-darwin)
#   VERSION       - App version (default: read from Cargo.toml)
#   OUTPUT_DIR    - Output directory (default: dist)

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Validate required environment variables
: "${SIGN_IDENTITY:?SIGN_IDENTITY is required}"

TARGET="${TARGET:-aarch64-apple-darwin}"
OUTPUT_DIR="${OUTPUT_DIR:-$PROJECT_ROOT/dist}"

# Get version from Cargo.toml if not provided
if [[ -z "${VERSION:-}" ]]; then
    VERSION=$(grep '^version = ' "$PROJECT_ROOT/Cargo.toml" | head -1 | sed 's/version = "\(.*\)"/\1/')
fi

APP_NAME="NativeDevtools"
BUNDLE_ID="xyz.primarch.native-devtools"
BINARY_NAME="native-devtools-mcp"

echo "Building $APP_NAME v$VERSION for $TARGET"

# Build the release binary
echo "Building release binary..."
cd "$PROJECT_ROOT"
cargo build --release --target "$TARGET"

# Prepare output directory
mkdir -p "$OUTPUT_DIR"
APP_BUNDLE="$OUTPUT_DIR/$APP_NAME.app"

# Copy app template
echo "Assembling app bundle..."
rm -rf "$APP_BUNDLE"
cp -R "$PROJECT_ROOT/packaging/macos/$APP_NAME.app" "$APP_BUNDLE"

# Ensure MacOS directory exists (git doesn't track empty directories)
mkdir -p "$APP_BUNDLE/Contents/MacOS"

# Copy binary into app bundle
cp "$PROJECT_ROOT/target/$TARGET/release/$BINARY_NAME" "$APP_BUNDLE/Contents/MacOS/"
chmod +x "$APP_BUNDLE/Contents/MacOS/$BINARY_NAME"

# Update Info.plist with version
sed -i '' "s/__VERSION__/$VERSION/g" "$APP_BUNDLE/Contents/Info.plist"

echo "App bundle assembled at: $APP_BUNDLE"

# Code sign with hardened runtime
echo "Signing app bundle with identity: $SIGN_IDENTITY"
codesign --force --options runtime --timestamp --sign "$SIGN_IDENTITY" "$APP_BUNDLE"

echo "Verifying signature..."
codesign --verify --verbose "$APP_BUNDLE"

echo "Build complete: $APP_BUNDLE"
