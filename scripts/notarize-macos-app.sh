#!/bin/bash
set -euo pipefail

# Sign, create DMG, notarize, and staple for macOS app
#
# Environment variables (required):
#   SIGN_IDENTITY         - Code signing identity (e.g., "Developer ID Application: ...")
#   NOTARY_API_KEY        - Path to App Store Connect API key (.p8 file)
#   NOTARY_API_KEY_ID     - App Store Connect API Key ID
#   NOTARY_API_ISSUER     - App Store Connect API Issuer ID
#
# Environment variables (optional):
#   APP_PATH              - Path to .app bundle (default: dist/NativeDevtools.app)
#   OUTPUT_DIR            - Output directory for DMG (default: dist)
#   VERSION               - Version string for DMG filename (default: read from app)

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Validate required environment variables
: "${SIGN_IDENTITY:?SIGN_IDENTITY is required}"
: "${NOTARY_API_KEY:?NOTARY_API_KEY is required}"
: "${NOTARY_API_KEY_ID:?NOTARY_API_KEY_ID is required}"
: "${NOTARY_API_ISSUER:?NOTARY_API_ISSUER is required}"

APP_PATH="${APP_PATH:-$PROJECT_ROOT/dist/NativeDevtools.app}"
OUTPUT_DIR="${OUTPUT_DIR:-$PROJECT_ROOT/dist}"
APP_NAME="$(basename "$APP_PATH" .app)"

# Get version from app if not provided
if [[ -z "${VERSION:-}" ]]; then
    VERSION=$(/usr/libexec/PlistBuddy -c "Print CFBundleShortVersionString" "$APP_PATH/Contents/Info.plist")
fi

DMG_NAME="${APP_NAME}-${VERSION}.dmg"
DMG_PATH="$OUTPUT_DIR/$DMG_NAME"

echo "Notarizing $APP_NAME v$VERSION"

# Ensure app is signed with hardened runtime
echo "Signing app bundle with hardened runtime..."
codesign --force --options runtime --timestamp --sign "$SIGN_IDENTITY" "$APP_PATH"

echo "Verifying signature..."
codesign --verify --verbose "$APP_PATH"

# Check with spctl
echo "Checking with Gatekeeper..."
spctl --assess --verbose "$APP_PATH" || echo "Warning: spctl check failed (expected before notarization)"

# Create DMG
echo "Creating DMG..."
rm -f "$DMG_PATH"

# Create a temporary directory for DMG contents
DMG_TEMP="$OUTPUT_DIR/dmg-temp"
rm -rf "$DMG_TEMP"
mkdir -p "$DMG_TEMP"

# Copy app to temp directory
cp -R "$APP_PATH" "$DMG_TEMP/"

# Create symlink to Applications
ln -s /Applications "$DMG_TEMP/Applications"

# Create DMG from temp directory
hdiutil create -volname "$APP_NAME" -srcfolder "$DMG_TEMP" -ov -format UDZO "$DMG_PATH"

# Clean up temp directory
rm -rf "$DMG_TEMP"

# Sign the DMG
echo "Signing DMG..."
codesign --force --timestamp --sign "$SIGN_IDENTITY" "$DMG_PATH"

# Submit for notarization
echo "Submitting DMG for notarization..."
xcrun notarytool submit "$DMG_PATH" \
    --key "$NOTARY_API_KEY" \
    --key-id "$NOTARY_API_KEY_ID" \
    --issuer "$NOTARY_API_ISSUER" \
    --wait

# Staple the notarization ticket to the DMG
echo "Stapling notarization ticket..."
xcrun stapler staple "$DMG_PATH"

# Verify stapling
echo "Verifying stapled DMG..."
xcrun stapler validate "$DMG_PATH"
spctl --assess --verbose "$DMG_PATH"

echo ""
echo "Success! Notarized DMG: $DMG_PATH"
