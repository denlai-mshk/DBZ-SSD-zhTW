#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ "$(uname -sm)" == "Darwin arm64" ]] || { echo "Run on an Apple Silicon Mac." >&2; exit 1; }
unset SSD_RECOMP_FILE
export MACOSX_DEPLOYMENT_TARGET=11.0
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$PWD/target}"
mkdir -p "$CARGO_TARGET_DIR"
# SDL uses Clang availability checks when targeting older macOS releases.
# Rust's linker omits Clang's runtime by default; link the active Xcode runtime explicitly.
clang_runtime=$(xcrun clang --print-libgcc-file-name)
[[ -f "$clang_runtime" ]] || { echo "Cannot locate the Xcode Clang runtime." >&2; exit 1; }
cargo rustc --locked --release -p ssd-app --bin ssd-recompiled --target aarch64-apple-darwin -- -C "link-arg=$clang_runtime"
out=$(mktemp -d "${CARGO_TARGET_DIR}/macos.XXXXXXXX")
trap 'rm -rf "$out"' EXIT
app="$out/DBZ SSD.app"
a="$app/Contents"
mkdir -p "$a/MacOS" "$a/Resources/fonts" "$a/Resources/licenses"
cp macos/Info.plist "$a/"
cp "$CARGO_TARGET_DIR/aarch64-apple-darwin/release/ssd-recompiled" "$a/MacOS/"
cp translations/zh-tw/*.tsv translations/zh-hk/*.tsv "$a/Resources/"
cp translations/zh-tw/fonts/LXGWWenKaiTC-Regular.ttf "$a/Resources/fonts/"
cp LICENSE LICENSE-ART.md NOTICE.md translations/zh-tw/licenses/* "$a/Resources/licenses/"
cp macos/README.md macos/README.en.md "$a/Resources/"
xcrun swiftc -module-cache-path "$CARGO_TARGET_DIR/swift-module-cache" -O -target arm64-apple-macosx11.0 macos/LauncherSupport.swift macos/Launcher.swift -o "$a/MacOS/DBZLauncher"
lipo -verify_arch arm64 "$a/MacOS/DBZLauncher"
lipo -verify_arch arm64 "$a/MacOS/ssd-recompiled"
codesign --force --sign - "$a/MacOS/ssd-recompiled"
codesign --force --sign - "$app"
codesign --verify --deep --strict "$app"
plutil -lint "$a/Info.plist"
# Keep one deterministic output; discard staging directories even if a build fails.
destination="$CARGO_TARGET_DIR/macos/DBZ SSD.app"
mkdir -p "$(dirname "$destination")"
if [[ -e "$destination" ]]; then
    mv "$destination" "$out/previous.app"
fi
if ! mv "$app" "$destination"; then
    [[ ! -d "$out/previous.app" ]] || mv "$out/previous.app" "$destination"
    exit 1
fi
echo "$destination"
