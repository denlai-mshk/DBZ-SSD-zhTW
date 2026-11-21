# Apple Silicon 原生 macOS 啟動器

繁體中文化：Ming Liu。支援台灣及香港譯名。需自備遊戲 ROM；不附 ROM、IPS 或 HD 美術包。[English instructions](README.en.md)

## 編譯

需要 Apple Silicon Mac、macOS 11 或以上、Apple 命令列工具（或 Xcode）、CMake 及 Rust。專案的 `rust-toolchain.toml` 已指定 Rust 1.99，請勿以 `RUSTUP_TOOLCHAIN=stable` 覆蓋它。在專案根目錄執行：

```sh
rustup target add --toolchain 1.99 aarch64-apple-darwin
bash macos/build.sh
```

編譯結果為 `target/macos/DBZ SSD.app`（設定 `CARGO_TARGET_DIR` 時位於該目錄之下）。重新編譯會替換這個應用程式；成功或失敗都會清理本次暫存目錄。舊版腳本產生的隨機目錄不會被自動刪除。此版本僅作本機臨時簽署，未使用 Developer ID，也未經 Apple 公證。最低版本同時寫入啟動器編譯目標及 `Info.plist`；未在所有舊版 macOS 上實測。

## 第一次啟動

開啟 `DBZ SSD.app`，選擇自行持有的 Klepto v1.02 英文修補版 ROM（`.sfc` 或 `.smc`），或日文 Rev 1 ROM 加上 `dbz102.ips`。遊戲引擎會驗證並快取正規化副本，不改動原始檔案。之後啟動不需重新選擇 ROM。啟動器會在遊戲啟動後短暫嘗試將其視窗帶到前景，不需「輔助使用」權限。

增強模式提供台灣及香港譯名，原版模式不提供繁中翻譯。啟動器不下載遊戲資料、HD 美術或更新。

## 資料與升級

存檔、ROM 快取、翻譯表、字型及 `Last Run.log` 固定存放於 `~/Library/Application Support/DBZ SSD`，不再隨版本改名。若此資料夾尚不存在而舊版 `DBZ SSD RC2` 資料夾存在，首次啟動會複製舊資料並保留原資料夾。若新舊資料夾都存在，使用新版資料夾，不自動合併或覆蓋存檔。複製失敗會停止啟動，不會使用不完整的遷移資料。

每次啟動會以內容比對 app 內的四份翻譯表及字型。內容有變就先備份舊檔到 `translation-backups`，再以原子寫入方式更新；內容相同不重寫。這也會取代自行編輯的翻譯，但舊內容仍在備份資料夾。存檔、狀態及 ROM 快取不屬於更新範圍。若快取損壞，請只將 `cache/rom/canonical.sfc` 移到別處，再重新啟動；不要移除存檔資料夾。

## 測試

`bash macos/test.sh` 會使用合成檔案測試升級、備份、重複啟動及 RC2 資料遷移，不需要 ROM，也不接觸真實使用者資料。實際遊玩、存讀檔及手把仍應另行測試。

翻譯表 © Ming，採 CC BY-NC-SA 4.0（https://creativecommons.org/licenses/by-nc-sa/4.0/）；共用 LXGW WenKai TC 字型採 SIL OFL 1.1。其他授權及致謝見根目錄 README、LICENSE 及 NOTICE.md。啟動器不包含遊戲資產。
