#!/bin/bash
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
if [[ "$(uname -s)" != Darwin ]]; then
    printf '%s\n' 'error: bundling requires macOS with Apple command-line tools.' >&2
    exit 1
fi
for tool in cargo python3 lipo codesign ditto shasum; do
    command -v "$tool" >/dev/null || { printf 'error: missing tool: %s\n' "$tool" >&2; exit 1; }
done
export MACOSX_DEPLOYMENT_TARGET=12.0
METADATA="$(cargo metadata --no-deps --format-version 1)"
VERSION="$(printf '%s' "$METADATA" | python3 -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"] == "alttab-macos"))')"
if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+([+-][0-9A-Za-z.-]+)?$ ]]; then
    printf 'error: invalid package version: %s\n' "$VERSION" >&2
    exit 1
fi
if [[ -n "${RELEASE_TAG:-}" && "$RELEASE_TAG" != "v$VERSION" ]]; then
    printf 'error: release tag %s does not match Cargo version v%s\n' "$RELEASE_TAG" "$VERSION" >&2
    exit 1
fi
TARGET_DIR="$(printf '%s' "$METADATA" | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"
for target in aarch64-apple-darwin x86_64-apple-darwin; do
    cargo build --locked --release --target "$target" -p alttab-macos --bin alttab-rs
done
APP="$ROOT/target/AltTab-rs.app"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" "$ROOT/dist"
lipo -create "$TARGET_DIR/aarch64-apple-darwin/release/alttab-rs" "$TARGET_DIR/x86_64-apple-darwin/release/alttab-rs" -output "$APP/Contents/MacOS/alttab-rs"
cp "$ROOT/macos/Info.plist" "$APP/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $VERSION" "$APP/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleVersion $VERSION" "$APP/Contents/Info.plist"
cp "$ROOT/assets/icon/AppIcon.icns" "$APP/Contents/Resources/AppIcon.icns"
cp "$ROOT/LICENSE" "$APP/Contents/Resources/LICENSE"
cp "$ROOT/docs/ATTRIBUTION.md" "$APP/Contents/Resources/ATTRIBUTION.md"
codesign --force --deep -s - "$APP"
ARCHIVE="$ROOT/dist/AltTab-rs-$VERSION.zip"
rm -f -- "$ARCHIVE"
ditto -c -k --keepParent "$APP" "$ARCHIVE"
shasum -a 256 "$ARCHIVE"
if [[ -n "${GITHUB_OUTPUT:-}" ]]; then
    SHA256="$(shasum -a 256 "$ARCHIVE")"
    SHA256="${SHA256%% *}"
    printf 'version=%s\nsha256=%s\narchive=%s\n' "$VERSION" "$SHA256" "$ARCHIVE" >> "$GITHUB_OUTPUT"
fi
