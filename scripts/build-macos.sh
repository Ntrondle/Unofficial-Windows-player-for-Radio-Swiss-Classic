#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
export MACOSX_DEPLOYMENT_TARGET=11.0
bundle="dist/Radio Swiss Classic.app"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
if [[ "${1:-}" == "--universal" ]]; then
  rustup target add aarch64-apple-darwin x86_64-apple-darwin
  cargo build --locked --release --target aarch64-apple-darwin
  cargo build --locked --release --target x86_64-apple-darwin
  lipo -create target/{aarch64,x86_64}-apple-darwin/release/radio-swiss-classic \
    -output "$bundle/Contents/MacOS/radio-swiss-classic"
else
  cargo build --locked --release
  cp target/release/radio-swiss-classic "$bundle/Contents/MacOS/"
fi
cp macos/Info.plist "$bundle/Contents/Info.plist"
cp assets/icon.icns "$bundle/Contents/Resources/"
codesign --force --sign - "$bundle"
codesign --verify --strict "$bundle"
printf 'Built: %s/%s\n' "$PWD" "$bundle"
