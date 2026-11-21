#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
mkdir -p target
work=$(mktemp -d "$PWD/target/macos-tests.XXXXXXXX")
trap 'rm -rf "$work"' EXIT
xcrun swiftc -module-cache-path "$work/modules" macos/LauncherSupport.swift macos/LauncherSupportTests.swift -o "$work/tests"
"$work/tests" "$work/data"
