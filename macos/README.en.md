# Native Apple Silicon macOS launcher

Traditional Chinese localisation: Ming Liu. Taiwan and Hong Kong names are supported. Supply your own ROM; no ROM, IPS or HD art is included. [繁體中文說明](README.md)

Install Apple Command Line Tools (or Xcode), CMake and Rust on an Apple Silicon Mac running macOS 11 or later. The repository pins Rust 1.99 in `rust-toolchain.toml`; do not override it with `RUSTUP_TOOLCHAIN=stable`.

```sh
rustup target add --toolchain 1.99 aarch64-apple-darwin
bash macos/build.sh
```

The output is `target/macos/DBZ SSD.app`, or under `CARGO_TARGET_DIR` if set. Rebuilding replaces that app and cleans this build's staging directory, including on failure. Old random output directories are not deleted automatically. Signing is local ad-hoc only, not Developer ID or notarisation. The minimum OS is set consistently in the launcher target and plist; not every older macOS release has been tested.

Open the app and choose your own Klepto v1.02 patched English ROM, or Japanese Rev 1 ROM plus `dbz102.ips`. The engine validates and caches a normalised copy; originals remain unchanged. Later launches reuse the cache. The launcher briefly requests foreground activation of the game without Accessibility permission. Enhanced mode offers Chinese translations; Classic mode does not. No game data, HD art or updates are downloaded.

Data has a stable location: `~/Library/Application Support/DBZ SSD`. When absent, an existing `DBZ SSD RC2` directory is copied there, preserving the original. If both exist, the stable directory wins; no saves are merged or overwritten. Failed migration stops launch rather than using an incomplete copy.

Every launch compares the four bundled translation tables and font by content. Changed files are backed up in `translation-backups` before atomic replacement, including local edits. Identical files are untouched. Saves, states and ROM cache are never refreshed. If the cache is damaged, move only `cache/rom/canonical.sfc` aside before relaunching.

Run `bash macos/test.sh` for synthetic resource-upgrade, backup and migration checks without ROM or real user data. Gameplay, save/load and controllers require separate testing.

Translation tables © Ming, CC BY-NC-SA 4.0; LXGW WenKai TC font, SIL OFL 1.1. See the root README, LICENSE and NOTICE.md. No game assets are contributed.
