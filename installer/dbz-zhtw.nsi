; 七龍珠Z 超賽亞傳說 繁中版：每使用者安裝檔（NSIS 3，Modern UI 2）。
; 只裝公開版啟動器 exe（不含 ROM、不含 hd-pack），不要求系統管理員。啟動器建置時設了更新來源，之後的新版才由啟動器
; 自己更新（換掉 exe 本身）；那樣的 exe 編安裝檔時加 -DAUTO_UPDATE。建置方式見同資料夾的 README.md：
;   makensis -DLAUNCHER_EXE=<公開版啟動器 exe> -DRELEASE=<NN> [-DDISPLAY_VERSION=<名稱>] -DOUTFILE=<輸出檔> installer/dbz-zhtw.nsi
; 命令列（README.md「命令列與結束碼」）：
;   安裝   /S /D=<資料夾> /ROM=<ROM 檔> /IPS=<IPS 檔> /LOG=<紀錄檔>
;   解除安裝 uninstall-dbz-zhtw.exe /S /DELETEDATA /LOG=<紀錄檔> _?=<安裝資料夾>（沒帶 /DELETEDATA 一律不刪玩家資料）
; 情境測試：installer/tests/run_wine_tests.py（用 Wine 實際執行安裝檔與解除安裝程式）

Unicode true
ManifestDPIAware true
RequestExecutionLevel user
SetCompressor /SOLID lzma

!ifndef LAUNCHER_EXE
  !error "用 -DLAUNCHER_EXE=<公開版啟動器 exe> 指定要裝的檔"
!endif
!ifndef RELEASE
  !error "用 -DRELEASE=<NN> 指定版號（launcher/RELEASE）"
!endif
; 對外的版本名稱（launcher/RELEASE_NAME，例如 RC1）：畫面、檔名與「新增或移除程式」顯示的都是它；
; 數字版號 RELEASE 只用在檔案版本的數字欄。沒給 -DDISPLAY_VERSION 時讀 launcher/RELEASE_NAME（makensis 在本腳本的
; 資料夾執行；!define /file 去掉檔尾換行）；給了就必須和那個檔一致
!define /file RELEASE_NAME_FILE "..\launcher\RELEASE_NAME"
!ifndef DISPLAY_VERSION
  !define DISPLAY_VERSION "${RELEASE_NAME_FILE}"
!else if "${DISPLAY_VERSION}" != "${RELEASE_NAME_FILE}"
  !error "-DDISPLAY_VERSION=${DISPLAY_VERSION} 與 launcher/RELEASE_NAME（${RELEASE_NAME_FILE}）不一致"
!endif
!if "${DISPLAY_VERSION}" == ""
  !error "launcher/RELEASE_NAME 是空的"
!endif

!ifndef OUTFILE
  !define OUTFILE "DBZ-SSD-zh-TW-${DISPLAY_VERSION}-setup.exe"
!endif

!define PRODUCT "七龍珠Z 超賽亞傳說 繁中版"
; 資料夾與登錄機碼用 ASCII；exe 名稱與啟動器的 OriginalFilename 相同（launcher/build.rs）
!define APP_ID "DBZ-SSD-zhTW"
!define EXE_NAME "DBZ_超賽亞傳說_繁中.exe"
!define EXE_STEM "DBZ_超賽亞傳說_繁中"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_ID}"
; 標記檔：資料夾歸屬的唯一依據（見 ReadMarker 上方的說明）
!define MARKER_NAME ".dbz-ssd-zhtw-install"
; 解除安裝程式的檔名：本程式專屬（之前的測試版叫 uninstall.exe，那個名字別的程式也會用）
!define UNINSTALLER "uninstall-dbz-zhtw.exe"
; CheckInstDir 試寫用的暫存檔（寫完就刪；分類時忽略它）
!define WRITE_TEST "dbz-zhtw-write-test.tmp"
; 本程式自己的機碼：InstallDir＝上次的安裝位置（InstallDirRegKey 讀它）
!define OWN_KEY "Software\${APP_ID}"
; 開始功能表資料夾與「下載 HD 美術包」捷徑名稱（README.md「之後再下載 HD 美術包」引用這個名稱）
!define SM_DIR "$SMPROGRAMS\${PRODUCT}"
!define HDPACK_LNK "下載 HD 美術包"
; 預放 ROM／IPS 的資料夾與 IPS 檔名（與啟動器約定：launcher/src/preset.rs 的 SETUP_DIR、ROM_NAMES、IPS_NAME）
!define SETUP_DIR "$INSTDIR\save\setup"
!define IPS_NAME "dbz102.ips"
; 啟動器每次開啟都會寫在 save\ 根層的同步標記（launcher/src/main.rs:548 SYNC_MARKER）：解除安裝的刪除清單有它；
; 選位置頁改裝到子資料夾時，用它判斷原本選的資料夾裡有沒有先前的存檔
!define SYNC_MARKER ".launcher-sync.toml"
; README.md「贊助」段的 Ko-fi 網址；權利人寫法依 README.md「授權與致謝」最後的聲明（原作由 BANDAI 發行，遊戲屬於各權利人）
!define KOFI_URL "https://ko-fi.com/ming888"

; 結束碼（README.md「命令列與結束碼」）
!define EXIT_BAD_DIR 10       ; 根目錄、Windows 資料夾、是檔案、寫不進去
; 11（別的程式的安裝位置）不再使用：解除安裝程式改用本程式專屬的檔名，資料夾裡有別人的 uninstall.exe 不影響安裝
!define EXIT_NOT_OURS_DIR 12  ; 資料夾已有其他檔案、不是本程式的（圖形介面會改裝到裡面的 ${APP_ID} 資料夾），或在另一個安裝的資料夾裡面
!define EXIT_BAD_ROM 13       ; /ROM、/IPS 不合輕量檢查
!define EXIT_WRITE_FAILED 14  ; 寫不進啟動器 exe 或解除安裝程式（例如遊戲正開著）；不寫「新增或移除程式」的項目。
                              ; 也用在安裝位置（或上層）的標記檔存在卻打不開：同樣是檔案被占用，關掉遊戲或防毒後再裝
!define EXIT_LEFTOVER 20      ; 解除安裝後清單內仍有刪不掉的東西（標記檔停在 installed，解除安裝程式與登記保留，可以再執行一次）
!define EXIT_NOT_OURS 21      ; 解除安裝的位置沒有 state=installed 的有效標記檔，或標記檔打不開無法確認（什麼都不做）

Name "${PRODUCT}"
Caption "${PRODUCT} ${DISPLAY_VERSION} 安裝程式"
UninstallCaption "${PRODUCT} 解除安裝"
BrandingText "${PRODUCT}  ${DISPLAY_VERSION}"
OutFile "${OUTFILE}"
; 預設裝在使用者自己的程式資料夾（一般使用者可寫入，自動更新才能換檔）；選安裝位置頁可以改，
; 改到的位置要能寫入（CheckInstDir）。重裝時帶出上次的位置（圖形介面與沒帶 /D= 的靜默安裝都是）：讀本程式自己的機碼
; ${OWN_KEY} 的 InstallDir。它和「新增或移除程式」的項目分開存：解除安裝且保留資料時不刪，下次才會回到同一個資料夾。
; 結尾的 \ 關掉 NSIS 的自動接名稱：沒有它時，按「瀏覽」選到的資料夾會被接上 \${APP_ID}（免安裝版資料夾就到不了原地升級）；
; 要不要改裝到子資料夾，由 DirLeave 的分類決定
InstallDir "$LOCALAPPDATA\Programs\${APP_ID}\"
InstallDirRegKey HKCU "${OWN_KEY}" "InstallDir"
; 詳細資訊預設收合，要看可按「顯示細節」
ShowInstDetails hide
ShowUninstDetails hide

VIProductVersion "${RELEASE}.0.0.0"
VIAddVersionKey /LANG=1028 "ProductName" "${PRODUCT}"
VIAddVersionKey /LANG=1028 "FileDescription" "${PRODUCT} 安裝程式"
VIAddVersionKey /LANG=1028 "FileVersion" "${DISPLAY_VERSION}"
VIAddVersionKey /LANG=1028 "ProductVersion" "${DISPLAY_VERSION}"
VIAddVersionKey /LANG=1028 "LegalCopyright" "ssd-recompiled © JackRushante/DBZ-Super-Saiya-Densetsu-Rust 及貢獻者，GPL-3.0-or-later；繁中譯文 © Ming Liu"

!include "MUI2.nsh"
!include "FileFunc.nsh"
!include "LogicLib.nsh"
!include "Sections.nsh"
; 外觀：頁面的樣子全在 ui\skin.nsh，不含任何頁面邏輯
!include "ui\skin.nsh"

!define MUI_ICON "..\launcher\icon\app_icon.ico"
!define MUI_UNICON "..\launcher\icon\app_icon.ico"
!define MUI_ABORTWARNING

; 外觀：原作對話框風格。天空藍底、米白底紅橙雙框與四角釘的面板、標題前的 ▶、
; 頭像框裡的超賽悟空（wizard.bmp／header.bmp 的美術）、深藍按鈕列；畫法見 ui\skin.nsh。
; MUI 自己的顏色設成面板的米白與深色字（歡迎頁、完成頁與頁首的文字都在米白面板上）；MUI 的頁首圖與側邊圖控制項
; 仍然保留（決定版面位置），顯示時由 skin.nsh 藏起來、改畫進頭像框。
!define MUI_BGCOLOR "${SKIN_CREAM}"
!define MUI_TEXTCOLOR "${SKIN_TEXT}"
!define MUI_CUSTOMFUNCTION_GUIINIT SkinInit
!define MUI_CUSTOMFUNCTION_UNGUIINIT un.SkinInit
!define MUI_HEADERIMAGE
!define MUI_HEADERIMAGE_RIGHT
!define MUI_HEADERIMAGE_BITMAP "header.bmp"
!define MUI_HEADERIMAGE_BITMAP_STRETCH "AspectFitHeight"
!define MUI_HEADERIMAGE_UNBITMAP "header.bmp"
!define MUI_HEADERIMAGE_UNBITMAP_STRETCH "AspectFitHeight"
!define MUI_WELCOMEFINISHPAGE_BITMAP "wizard.bmp"
!define MUI_WELCOMEFINISHPAGE_BITMAP_STRETCH "AspectFitHeight"
!define MUI_INSTFILESPAGE_COLORS "${SKIN_TEXT} ${SKIN_CREAM}"
!define MUI_FINISHPAGE_LINK_COLOR "${SKIN_TITLE}"
; 佈景主題下的核取方塊不吃 SetCtlColors（MUI2 Pages/Finish.nsh 的 bug #443 註解），深底上會是黑字；
; 這個 MUI2 內建開關讓完成頁的核取方塊改用傳統外觀，文字才會是 MUI_TEXTCOLOR。
!define MUI_FORCECLASSICCONTROLS

; 標題在「歡迎安裝」後換行（字沒有改）
!define MUI_WELCOMEPAGE_TITLE "歡迎安裝$\r$\n${PRODUCT}"
; 歡迎頁最後一句：啟動器有沒有自動更新由它的建置決定（launcher/build.rs：DBZ_UPDATE_MANIFEST_URL 與 DBZ_UPDATE_PUBLIC_KEY
; 都設了才有）。沒有 -DAUTO_UPDATE 時不提自動更新，改說用新版安裝程式再裝一次（重裝只覆蓋安裝程式自己放的檔，
; save\ 與 %LOCALAPPDATA%\dbz-ssd-zhtw 不動）
!ifdef AUTO_UPDATE
  !define UPDATE_NOTE "之後出新版，啟動器開啟時會詢問是否更新，不必再執行安裝程式。"
!else
  !define UPDATE_NOTE "之後出新版時，下載新版的安裝程式再裝一次即可，存檔與設定都會保留。"
!endif
; 第一段（$WelcomeLead）在 .onInit 依上次的安裝位置決定（PrevInstall）：那裡已經裝著本程式時說「偵測到已安裝的 RC2，
; 這會原地升級為 RC3」，同一版說「在原位置重新安裝」，其餘照原本的「這會安裝 RC3」。執行時展開（MUI2 的歡迎頁用
; nsDialogs 建文字，變數在執行時代入）
Var WelcomeLead
; 第三段（ROM 與 HD 美術包）手動換行，每行最多 15 個全形字，Windows 不會在詞中間自動折行。「不下載也能玩，畫面是
; 原版像素」與啟動器的詢問文字一致（launcher/src/hd_offer.rs：不下載也能玩，畫面用原版像素）
!define MUI_WELCOMEPAGE_TEXT "$WelcomeLead$\r$\n$\r$\n只裝給目前的使用者，不需要系統管理員權限，並在開始功能表和桌面建立捷徑。$\r$\n$\r$\n安裝檔不含遊戲 ROM 和 HD 美術包。$\r$\n第一次開啟時請選你自己的 ROM。$\r$\n高畫質的畫面要靠 HD 美術包，$\r$\n可以選擇要不要下載；$\r$\n不下載也能玩，畫面是原版像素。$\r$\n$\r$\n${UPDATE_NOTE}"
!define MUI_PAGE_CUSTOMFUNCTION_SHOW SkinPage
!insertmacro MUI_PAGE_WELCOME

; 選安裝位置。內頁本體是系統色（MUI_BGCOLOR 只管頁首、歡迎頁、完成頁），與安裝中頁一致。
; DirVerify leave（MUI_DIRECTORYPAGE_VERIFYONLEAVE）：路徑無效或空間不足時「下一步」鈕不變灰，由 DirLeave 說明原因
!define MUI_PAGE_HEADER_TEXT "選擇安裝位置"
!define MUI_PAGE_HEADER_SUBTEXT "選擇要把${PRODUCT}裝在哪個資料夾。"
!define MUI_DIRECTORYPAGE_TEXT_TOP "遊戲會裝在下面的資料夾。要換位置，請按「瀏覽」或直接修改路徑。$\r$\n$\r$\n存檔、ROM 複本與 HD 美術包都會放在這個資料夾裡，所以要選目前的使用者可以寫入的位置；不確定的話用預設位置就好。"
!define MUI_DIRECTORYPAGE_TEXT_DESTINATION "安裝資料夾"
!define MUI_DIRECTORYPAGE_VERIFYONLEAVE
!define MUI_PAGE_CUSTOMFUNCTION_SHOW SkinPage
!define MUI_PAGE_CUSTOMFUNCTION_LEAVE DirLeave
!insertmacro MUI_PAGE_DIRECTORY

; 選遊戲 ROM 與 IPS（nsDialogs，RomPageShow／RomPageLeave）：可略過。選了的檔在安裝時複製到 save\setup\，
; 啟動器第一次開啟時讀它、走原本的驗證（與啟動器約定的位置與檔名，見 CheckRomInputs 上方）
Page custom RomPageShow RomPageLeave

; HD 高畫質美術包（nsDialogs，HdPageShow／HdPageLeave）：一張有無美術包的對照圖、為什麼需要它，以及「安裝完成後下載」
; 的核取方塊（預設不勾）。勾了的話完成時跑啟動器 --hd-pack（FinishRun／FinishLeave）。安裝位置已經有美術包時只告知
Page custom HdPageShow HdPageLeave

!define MUI_PAGE_HEADER_TEXT "正在安裝"
!define MUI_PAGE_HEADER_SUBTEXT "正在把${PRODUCT}裝到你的電腦，請稍候。"
!define MUI_PAGE_CUSTOMFUNCTION_SHOW SkinPage
!define MUI_PAGE_CUSTOMFUNCTION_LEAVE SkinInstDone
!insertmacro MUI_PAGE_INSTFILES

!define MUI_FINISHPAGE_TITLE "安裝完成"
; 有核取方塊時說明文字框只有 40u 高（MUI2 Pages/Finish.nsh 的 MUI_FINISHPAGE_TEXT_HEIGHT_BUTTONS），
; Ko-fi 段落放不下：用 MUI_FINISHPAGE_TEXT_LARGE（60u），第一段縮成一行、兩段之間不空行
!define MUI_FINISHPAGE_TEXT_LARGE
; 手動換行：Windows 會在任兩個中文字之間自動折行，把「完全自／願」這類詞切開。每行最多 15 個全形字（實機這個寬度
; 一行約 19 個），在標點或詞與詞之間斷，自動折行就不會發生
!define MUI_FINISHPAGE_TEXT "遊戲已經裝好，可以從開始功能表$\r$\n或桌面的捷徑開啟。$\r$\n如果你喜歡這個繁中版，$\r$\n想支持背後投入的時間與工具，$\r$\n可以請我喝杯咖啡。完全自願：$\r$\n贊助支持的是這個專案的製作，$\r$\n不是遊戲本身，遊戲的權利屬於 $\r$\nBANDAI 等各權利人。"
!define MUI_FINISHPAGE_LINK "Ko-fi（${KOFI_URL}）"
!define MUI_FINISHPAGE_LINK_LOCATION "${KOFI_URL}"
!define MUI_FINISHPAGE_RUN
; 測試用（tests/run_wine_tests.py g11、g12）：-DTEST_FINISH_BOX_PAD=<字串> 接在核取方塊的文字後面（完成頁的
; 「立即開啟遊戲」、HD 美術包頁的「安裝完成後下載」），讓它們在 Wine 的黑體 TC 下也一定折成多行（Windows 的新細明體
; 比較寬，舊版完成頁的「下載 HD 美術包（之後也可從開始功能表）」在實機就折行、被切掉一半）。發行版不定義
!ifndef TEST_FINISH_BOX_PAD
  !define TEST_FINISH_BOX_PAD ""
!endif
!define MUI_FINISHPAGE_RUN_TEXT "立即開啟遊戲${TEST_FINISH_BOX_PAD}"
!define MUI_FINISHPAGE_RUN_FUNCTION FinishRun
; 「下載 HD 美術包」在 HD 美術包頁勾（$HdWanted），完成頁不再有第二個核取方塊。沒勾「立即開啟遊戲」時由
; FinishLeave 跑 --hd-pack；有勾時 FinishRun 先等 --hd-pack 跑完再開遊戲（MUI 先叫 LEAVE 自訂函式、再叫 RUN 函式）
!define MUI_PAGE_CUSTOMFUNCTION_SHOW SkinPage
!define MUI_PAGE_CUSTOMFUNCTION_LEAVE FinishLeave
!insertmacro MUI_PAGE_FINISH

!define MUI_UNCONFIRMPAGE_TEXT_TOP "會移除安裝程式放的啟動器、捷徑與「新增或移除程式」的登記。$\r$\n$\r$\n存檔、ROM 設定與美術覆蓋圖（安裝資料夾裡的 save 資料夾）、hd-pack.zip，以及 %LOCALAPPDATA%\dbz-ssd-zhtw 裡的遊戲本體、設定與紀錄預設都會保留；下一步可以選擇一併刪除。安裝資料夾裡還有東西時，會留下一個小標記檔 ${MARKER_NAME}，之後在同一個位置重裝就認得。"
!define MUI_PAGE_CUSTOMFUNCTION_SHOW un.SkinPage
!insertmacro MUI_UNPAGE_CONFIRM
; 選擇要移除的項目（元件頁）：「程式本體」固定勾選；「同時刪除…」是 Section /o，預設不勾。
; 靜默解除安裝只有帶 /DELETEDATA 才會勾（un.onInit）。勾了之後離開本頁時再確認一次（un.DataLeave，預設按鈕是「否」）
!define MUI_COMPONENTSPAGE_NODESC
!define MUI_PAGE_HEADER_TEXT "選擇要移除的項目"
!define MUI_PAGE_HEADER_SUBTEXT "預設只移除程式本體，你的資料都會保留。"
!define MUI_COMPONENTSPAGE_TEXT_TOP "預設只移除程式本體。要連同存檔、ROM 設定、HD 美術包與遊戲設定一起刪除，請勾選第二項（無法復原）。存檔包含啟動器從原版遊戲（%LOCALAPPDATA%\dbz-ssd-recompiled）搬過來的存檔，會一併刪除。只刪本程式寫的檔案與資料夾：save 資料夾裡其他東西、遊戲截圖、環境變數 SSD_DATA_DIR 指定的資料夾都不會刪。"
!define MUI_COMPONENTSPAGE_TEXT_COMPLIST "要移除的項目："
!define MUI_PAGE_CUSTOMFUNCTION_SHOW un.DataShow
!define MUI_PAGE_CUSTOMFUNCTION_LEAVE un.DataLeave
!insertmacro MUI_UNPAGE_COMPONENTS
!define MUI_PAGE_CUSTOMFUNCTION_SHOW un.SkinPage
!define MUI_PAGE_CUSTOMFUNCTION_LEAVE un.SkinInstDone
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "TradChinese"

; ======================================================================
; 共用函式（安裝程式與解除安裝程式各一份：巨集展開成 Name 與 un.Name）
; ======================================================================

Var LogPath

; Push 訊息；Call [un.]Log：附加一行到 $LogPath（UTF-16LE），同時顯示在詳細資訊
!macro LOG_FUNC UN
Function ${UN}Log
  Exch $0
  Push $1
  Push $2
  DetailPrint "$0"
  ${If} $LogPath != ""
    ClearErrors
    FileOpen $1 "$LogPath" a
    ${IfNot} ${Errors}
      FileSeek $1 0 END $2
      ${If} $2 == 0
        FileWriteWord $1 0xFEFF
      ${EndIf}
      FileWriteUTF16LE $1 "$0$\r$\n"
      FileClose $1
    ${EndIf}
  ${EndIf}
  Pop $2
  Pop $1
  Pop $0
FunctionEnd
!macroend
!insertmacro LOG_FUNC ""
!insertmacro LOG_FUNC "un."

; Push 參數字串；Call [un.]NormParams；Pop 得到把整個加了引號的選項改寫成值加引號的結果：
; "/ROM=C:\a b\x.sfc" → /ROM="C:\a b\x.sfc"。值含空白時 PowerShell 會整個加引號傳，GetOptions 讀不到那種寫法
!macro NORM_PARAMS_FUNC UN
Function ${UN}NormParams
  Exch $0
  Push $1
  Push $2
  Push $3
  Push $4
  Push $5
  Push $6
  StrCpy $1 ""
  StrCpy $2 0
  StrCpy $4 " "
  ${Do}
    StrCpy $3 $0 1 $2
    ${If} $3 == ""
      ${ExitDo}
    ${EndIf}
    ${If} $3 == "$\""
    ${AndIf} $4 == " "
      IntOp $5 $2 + 1
      StrCpy $6 $0 1 $5
      ${If} $6 == "/"
        ; 選項名稱到 "=" 為止，中間沒有空白或引號才算
        ${Do}
          IntOp $5 $5 + 1
          StrCpy $6 $0 1 $5
          ${If} $6 == "="
          ${OrIf} $6 == ""
          ${OrIf} $6 == " "
          ${OrIf} $6 == "$\""
            ${ExitDo}
          ${EndIf}
        ${Loop}
        ${If} $6 == "="
          IntOp $6 $5 - $2
          IntOp $2 $2 + 1
          StrCpy $6 $0 $6 $2
          StrCpy $1 "$1$6$\""
          IntOp $2 $5 + 1
          StrCpy $4 "="
          ${Continue}
        ${EndIf}
      ${EndIf}
    ${EndIf}
    StrCpy $1 "$1$3"
    StrCpy $4 $3
    IntOp $2 $2 + 1
  ${Loop}
  StrCpy $0 $1
  Pop $6
  Pop $5
  Pop $4
  Pop $3
  Pop $2
  Pop $1
  Exch $0
FunctionEnd
!macroend
!insertmacro NORM_PARAMS_FUNC ""
!insertmacro NORM_PARAMS_FUNC "un."

; Push 參數；Push 選項（例如 "/LOG="）；Call [un.]GetOpt；Pop 得到值（沒有這個選項時是空字串）。
; 選項前面要是空白或字串開頭，不分大小寫；值以 " 開頭時取到下一個 "，否則取到下一個「空白＋/」或字串結尾（去掉結尾空白）。
; 不用 GetOptions：它把撇號與 ` 也當引號，值裡有撇號（C:\Users\O'Neil\…）時會讀錯，連後面的參數一起吞掉
!macro GET_OPT_FUNC UN
Function ${UN}GetOpt
  Exch $1
  Exch
  Exch $0
  Push $2
  Push $3
  Push $4
  Push $5
  Push $6
  StrCpy $0 " $0"
  StrLen $2 $1
  StrLen $3 $0
  StrCpy $4 1
  StrCpy $6 ""
  ${Do}
    ${If} $4 >= $3
      ${ExitDo}
    ${EndIf}
    StrCpy $5 $0 $2 $4
    ${If} $5 == $1
      IntOp $5 $4 - 1
      StrCpy $5 $0 1 $5
      ${If} $5 == " "
        IntOp $4 $4 + $2
        StrCpy $5 $0 1 $4
        ${If} $5 == "$\""
          IntOp $4 $4 + 1
          ${Do}
            StrCpy $5 $0 1 $4
            ${If} $5 == ""
            ${OrIf} $5 == "$\""
              ${ExitDo}
            ${EndIf}
            StrCpy $6 "$6$5"
            IntOp $4 $4 + 1
          ${Loop}
        ${Else}
          ${Do}
            StrCpy $5 $0 2 $4
            ${If} $5 == ""
            ${OrIf} $5 == " /"
              ${ExitDo}
            ${EndIf}
            StrCpy $5 $0 1 $4
            StrCpy $6 "$6$5"
            IntOp $4 $4 + 1
          ${Loop}
          ${Do}
            StrCpy $5 $6 1 -1
            ${If} $5 != " "
              ${ExitDo}
            ${EndIf}
            StrCpy $6 $6 -1
          ${Loop}
        ${EndIf}
        ${ExitDo}
      ${EndIf}
    ${EndIf}
    IntOp $4 $4 + 1
  ${Loop}
  StrCpy $0 $6
  Pop $6
  Pop $5
  Pop $4
  Pop $3
  Pop $2
  Exch $0
  Exch
  Pop $1
FunctionEnd
!macroend
!insertmacro GET_OPT_FUNC ""
!insertmacro GET_OPT_FUNC "un."

; Push 路徑；Call [un.]PathState；Pop 得到 "none"（不存在）、"dir"、"file"、"other"（存在但讀不到屬性，例如沒有權限）
!macro PATH_STATE_FUNC UN
Function ${UN}PathState
  Exch $0
  Push $1
  Push $2
  System::Call 'kernel32::GetFileAttributesW(w r0) i .r1 ?e'
  Pop $2
  ${If} $1 = -1
    ${If} $2 = 2
    ${OrIf} $2 = 3
      StrCpy $0 "none"
    ${Else}
      StrCpy $0 "other"
    ${EndIf}
  ${Else}
    IntOp $1 $1 & 0x10
    ${If} $1 <> 0
      StrCpy $0 "dir"
    ${Else}
      StrCpy $0 "file"
    ${EndIf}
  ${EndIf}
  Pop $2
  Pop $1
  Exch $0
FunctionEnd
!macroend
!insertmacro PATH_STATE_FUNC ""
!insertmacro PATH_STATE_FUNC "un."

; Push 路徑；Call [un.]FileId；Pop 得到 "磁碟序號:檔案編號高:低"（打不開時是空字串）。同一個檔或資料夾，
; 不論用 8.3 短名、大小寫不同、subst 磁碟或連接點去開，得到的都一樣
!macro FILE_ID_FUNC UN
Function ${UN}FileId
  Exch $0
  Push $1
  Push $2
  Push $3
  Push $4
  Push $5
  ; FILE_READ_ATTRIBUTES、共用讀寫刪、OPEN_EXISTING、FILE_FLAG_BACKUP_SEMANTICS（資料夾也打得開）
  System::Call 'kernel32::CreateFileW(w r0, i 0x80, i 7, p 0, i 3, i 0x02000000, p 0) p .r1'
  ${If} $1 P<> -1
    ; BY_HANDLE_FILE_INFORMATION：13 個 DWORD；第 8 個是磁碟序號，第 12、13 個是檔案編號
    System::Call '*(i,i,i,i,i,i,i,i,i,i,i,i,i) p .r2'
    System::Call 'kernel32::GetFileInformationByHandle(p r1, p r2) i .r3'
    ${If} $3 <> 0
      System::Call '*$2(i,i,i,i,i,i,i,i.r3,i,i,i,i.r4,i.r5)'
      ${If} $4 = 0
      ${AndIf} $5 = 0
        ; 檔案編號高低兩段都是 0：這個檔案系統沒給編號，不能拿來比（不靠磁碟序號判斷：Wine 下序號有時是 0）
        StrCpy $0 ""
      ${Else}
        StrCpy $0 "$3:$4:$5"
      ${EndIf}
    ${Else}
      StrCpy $0 ""
    ${EndIf}
    System::Free $2
    System::Call 'kernel32::CloseHandle(p r1)'
  ${Else}
    StrCpy $0 ""
  ${EndIf}
  Pop $5
  Pop $4
  Pop $3
  Pop $2
  Pop $1
  Exch $0
FunctionEnd
!macroend
!insertmacro FILE_ID_FUNC ""
!insertmacro FILE_ID_FUNC "un."

; Push 路徑；Call [un.]NormPath；Pop 得到完整路徑、長檔名（存在時）、去掉結尾反斜線的結果
!macro NORM_PATH_FUNC UN
Function ${UN}NormPath
  Exch $0
  Push $1
  Push $2
  ${If} $0 != ""
    System::Call 'kernel32::GetFullPathNameW(w r0, i ${NSIS_MAX_STRLEN}, w .r1, p 0) i .r2'
    ${If} $2 > 0
    ${AndIf} $2 < ${NSIS_MAX_STRLEN}
      StrCpy $0 $1
    ${EndIf}
    System::Call 'kernel32::GetLongPathNameW(w r0, w .r1, i ${NSIS_MAX_STRLEN}) i .r2'
    ${If} $2 > 0
    ${AndIf} $2 < ${NSIS_MAX_STRLEN}
      StrCpy $0 $1
    ${EndIf}
    StrCpy $1 $0 1 -1
    ${If} $1 == "\"
      StrCpy $0 $0 -1
    ${EndIf}
  ${EndIf}
  Pop $2
  Pop $1
  Exch $0
FunctionEnd
!macroend
!insertmacro NORM_PATH_FUNC ""
!insertmacro NORM_PATH_FUNC "un."

; Push 甲；Push 乙；Call [un.]SamePath；Pop 得到 "1"（同一個檔或資料夾）或 "0"。
; 所有「是不是同一個」的判斷都走這裡：兩邊都存在時比磁碟序號與檔案編號（FileId），
; 否則比正規化後的路徑（NormPath，StrCmp 不分大小寫）
!macro SAME_PATH_FUNC UN
Function ${UN}SamePath
  Exch $1
  Exch
  Exch $0
  Push $2
  Push $3
  ${If} $0 == ""
  ${OrIf} $1 == ""
    StrCpy $0 "0"
  ${Else}
    Push $0
    Call ${UN}FileId
    Pop $2
    Push $1
    Call ${UN}FileId
    Pop $3
    ${If} $2 != ""
    ${AndIf} $3 != ""
      ${If} $2 == $3
        StrCpy $0 "1"
      ${Else}
        StrCpy $0 "0"
      ${EndIf}
    ${Else}
      Push $0
      Call ${UN}NormPath
      Pop $2
      Push $1
      Call ${UN}NormPath
      Pop $3
      ${If} $2 == $3
        StrCpy $0 "1"
      ${Else}
        StrCpy $0 "0"
      ${EndIf}
    ${EndIf}
  ${EndIf}
  Pop $3
  Pop $2
  Exch $0
  Exch
  Pop $1
FunctionEnd
!macroend
!insertmacro SAME_PATH_FUNC ""
!insertmacro SAME_PATH_FUNC "un."

; Call [un.]RegisteredHere；Pop 得到 "1"：「新增或移除程式」登記的 InstallLocation 就是 $INSTDIR（SamePath）
!macro REGISTERED_HERE_FUNC UN
Function ${UN}RegisteredHere
  Push $0
  ReadRegStr $0 HKCU "${UNINSTALL_KEY}" "InstallLocation"
  ${If} $0 == ""
    StrCpy $0 "0"
  ${Else}
    Push $0
    Push $INSTDIR
    Call ${UN}SamePath
    Pop $0
  ${EndIf}
  Exch $0
FunctionEnd
!macroend
!insertmacro REGISTERED_HERE_FUNC ""
!insertmacro REGISTERED_HERE_FUNC "un."

; 標記檔 ${MARKER_NAME}：資料夾歸屬的唯一依據，先進後出（安裝時第一個寫，解除安裝時最後才處理）。
; 一行一個 key=value：app=DBZ-SSD-zhTW（有這行才有效）、version=、installed=、state=installed|removed。
; state=removed：解除安裝過、資料夾裡還有東西（例如保留的存檔），重裝時仍認得是本程式的資料夾。
; 之前測試版寫的其他欄位（uninstaller=、save=）讀到就忽略；沒有 state= 的舊標記當成 installed
Var MkValid       ; "1"：讀到的標記檔有效
Var MkState       ; state= 的值（有效而沒有這行時是 "installed"）
Var MkInstalled   ; installed= 的值（WriteMarker 照寫回去）
Var MkUnreadable  ; "1"：標記檔存在卻打不開（權限或共用衝突）

; Push 資料夾；Call [un.]ReadMarker：讀那個資料夾的標記檔，設定 $MkValid、$MkState、$MkInstalled
!macro READ_MARKER_FUNC UN
Function ${UN}ReadMarker
  Exch $0
  Push $1
  Push $2
  StrCpy $MkValid "0"
  StrCpy $MkState ""
  StrCpy $MkInstalled ""
  StrCpy $MkUnreadable "0"
  ClearErrors
  FileOpen $1 "$0\${MARKER_NAME}" r
  ; 開檔成敗只能判斷一次：${Errors}（IfErrors）判斷完就清掉錯誤旗標，之後再問 ${Errors} 永遠是「沒錯誤」，
  ; 所以用同一組 If／Else，讀檔迴圈只在開檔成功時進入
  ${If} ${Errors}
    ${If} ${FileExists} "$0\${MARKER_NAME}"
      StrCpy $MkUnreadable "1"
    ${EndIf}
  ${Else}
    ${Do}
      ClearErrors
      FileRead $1 $0
      ${If} ${Errors}
        ${ExitDo}
      ${EndIf}
      ${Do}
        StrCpy $2 $0 1 -1
        ${If} $2 != "$\r"
        ${AndIf} $2 != "$\n"
          ${ExitDo}
        ${EndIf}
        StrCpy $0 $0 -1
      ${Loop}
      StrCpy $2 $0 6
      ${If} $0 S== "app=${APP_ID}"
        StrCpy $MkValid "1"
      ${ElseIf} $2 S== "state="
        StrCpy $MkState $0 "" 6
      ${Else}
        StrCpy $2 $0 10
        ${If} $2 S== "installed="
          StrCpy $MkInstalled $0 "" 10
        ${EndIf}
      ${EndIf}
    ${Loop}
    FileClose $1
  ${EndIf}
  ${If} $MkValid == "1"
  ${AndIf} $MkState == ""
    StrCpy $MkState "installed"
  ${EndIf}
  Pop $2
  Pop $1
  Pop $0
FunctionEnd
!macroend
!insertmacro READ_MARKER_FUNC ""
!insertmacro READ_MARKER_FUNC "un."

; Push state（installed／removed）；Call [un.]WriteMarker；Pop 得到 "ok"／"fail"：寫 $INSTDIR 的標記檔並設成隱藏
; （installed= 照 $MkInstalled 寫；舊欄位不再寫）
!macro WRITE_MARKER_FUNC UN
Function ${UN}WriteMarker
  Exch $0
  Push $1
  SetFileAttributes "$INSTDIR\${MARKER_NAME}" NORMAL
  ClearErrors
  FileOpen $1 "$INSTDIR\${MARKER_NAME}" w
  ${If} ${Errors}
    StrCpy $0 "fail"
  ${Else}
    FileWrite $1 "app=${APP_ID}$\r$\nversion=v${RELEASE}$\r$\ninstalled=$MkInstalled$\r$\nstate=$0$\r$\n"
    FileClose $1
    ; 隱藏：不讓人以為是可以順手刪的雜檔（刪了重裝會被當成別人的資料夾）。改寫前先清掉屬性，隱藏檔才寫得進去
    SetFileAttributes "$INSTDIR\${MARKER_NAME}" HIDDEN
    StrCpy $0 "ok"
  ${EndIf}
  Pop $1
  Exch $0
FunctionEnd
!macroend
!insertmacro WRITE_MARKER_FUNC ""
!insertmacro WRITE_MARKER_FUNC "un."

; 「新增或移除程式」的項目（只寫目前使用者的 HKCU）：指向一個安裝資料夾的指標。安裝時寫；解除安裝時先刪，
; 有殘留時用同一份寫回（版號與大小用刪除前讀到的）
; 開始功能表：一個資料夾放「遊戲」與「下載 HD 美術包」（--hd-pack 只下載美術包、不開始遊戲）；桌面只放遊戲。
; 安裝時建；解除安裝有殘留、把項目寫回時也一併補回
!macro WRITE_SHORTCUTS
  CreateDirectory "${SM_DIR}"
  CreateShortcut "${SM_DIR}\${PRODUCT}.lnk" "$INSTDIR\${EXE_NAME}" "" "$INSTDIR\${EXE_NAME}" 0
  CreateShortcut "${SM_DIR}\${HDPACK_LNK}.lnk" "$INSTDIR\${EXE_NAME}" "--hd-pack" "$INSTDIR\${EXE_NAME}" 0 SW_SHOWNORMAL "" "下載 HD 美術包（上游 v0.3.9 的 hd-pack.zip），不開始遊戲"
  CreateShortcut "$DESKTOP\${PRODUCT}.lnk" "$INSTDIR\${EXE_NAME}" "" "$INSTDIR\${EXE_NAME}" 0
!macroend

!macro WRITE_UNINSTALL_ENTRY _VER _SIZE
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "${PRODUCT}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${_VER}"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "Publisher" "Ming Liu"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\${EXE_NAME},0"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\${UNINSTALLER}"'
  WriteRegStr HKCU "${UNINSTALL_KEY}" "QuietUninstallString" '"$INSTDIR\${UNINSTALLER}" /S'
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
  WriteRegDWORD HKCU "${UNINSTALL_KEY}" "EstimatedSize" "${_SIZE}"
!macroend

; ======================================================================
; 安裝程式：檢查（圖形介面的 leave 函式與靜默安裝的 .onInit 呼叫同一份）
; ======================================================================

; 檢查結果：$CheckKind＝"ok"／"block"（擋下）／"foreign"（不是本程式的資料夾，圖形介面改裝到子資料夾）；
; $CheckMsg＝給人看的說明；$CheckCode＝靜默時的結束碼
Var CheckKind
Var CheckMsg
Var CheckCode
; ClassifyInstDir 的結果（依序，第一個符合的生效）：$DirClass＝"file"（是檔案）／"inside"（在另一個安裝的資料夾裡面，
;   $DirInside＝那個安裝）／"ours"（有效標記檔，不論 state）／"empty"（不存在，或除了 desktop.ini、Thumbs.db 沒有別的）
;   ／"locked"（這裡或上層的標記檔存在卻打不開，$DirLocked＝那個資料夾：和解除安裝端一樣當成那份安裝還在，不裝）
;   ／"portable"（沒有標記檔，免安裝版的啟動器 exe 在）／"foreign"（其餘）
Var DirClass
Var DirInside        ; DirClass == "inside" 時，標記檔所在的上層資料夾
Var DirInsideState   ; 那個標記檔的 state
Var DirUnlistable    ; "1"：資料夾列不出內容或讀不到屬性（歸到 foreign）
Var DirLocked        ; DirClass == "locked" 時，標記檔打不開的那個資料夾

; Push 檔案路徑；Push 欄位名（ProductName、InternalName…）；Call VersionString；Pop 得到版本資源裡那個欄位的值
; （第一組語言與字碼頁；沒有版本資源或沒有這個欄位時是空字串）
Function VersionString
  Exch $1
  Exch
  Exch $0
  Push $2
  Push $3
  Push $4
  Push $5
  Push $6
  StrCpy $6 ""
  System::Call 'version::GetFileVersionInfoSizeW(w r0, *i .r5) i .r2'
  ${If} $2 > 0
    System::Alloc $2
    Pop $3
    System::Call 'version::GetFileVersionInfoW(w r0, i 0, i r2, p r3) i .r5'
    ${If} $5 <> 0
      System::Call 'version::VerQueryValueW(p r3, w "\VarFileInfo\Translation", *p .r4, *i .r5) i .r2'
      ${If} $2 <> 0
      ${AndIf} $5 >= 4
        System::Call '*$4(&i2 .r2, &i2 .r5)'
        IntFmt $2 "%04x" $2
        IntFmt $5 "%04x" $5
        System::Call 'version::VerQueryValueW(p r3, w "\StringFileInfo\$2$5\$1", *p .r4, *i .r5) i .r2'
        ${If} $2 <> 0
          System::Call '*$4(&w256 .r6)'
        ${EndIf}
      ${EndIf}
    ${EndIf}
    System::Free $3
  ${EndIf}
  StrCpy $0 $6
  Pop $6
  Pop $5
  Pop $4
  Pop $3
  Pop $2
  Exch $0
  Exch
  Pop $1
FunctionEnd

; Push exe 路徑；Call IsOurLauncher；Pop 得到 "1"：版本資源的 InternalName 是 dbz-zhtw-launcher、OriginalFilename 是
; ${EXE_NAME}（launcher/build.rs 寫入的兩個欄位）。只看檔名不算
Function IsOurLauncher
  Exch $0
  Push $1
  Push $0
  Push "InternalName"
  Call VersionString
  Pop $1
  ${If} $1 S== "dbz-zhtw-launcher"
    Push $0
    Push "OriginalFilename"
    Call VersionString
    Pop $1
    ${If} $1 S== "${EXE_NAME}"
      StrCpy $0 "1"
    ${Else}
      StrCpy $0 "0"
    ${EndIf}
  ${Else}
    StrCpy $0 "0"
  ${EndIf}
  Pop $1
  Exch $0
FunctionEnd

; 安裝位置的分類（見 $DirClass 的說明）。歸屬只看標記檔；資料夾裡有沒有別人的 uninstall.exe 不影響
Function ClassifyInstDir
  Push $0
  Push $1
  Push $2
  ; 在另一個安裝的資料夾裡面：上層的有效標記檔是 state=installed → 整棵擋下；是 state=removed（解除過、保留了東西）
  ; → 只擋它的 save\ 底下（那裡是保留的存檔），其餘放行
  StrCpy $DirInside ""
  StrCpy $DirInsideState ""
  StrCpy $DirUnlistable "0"
  StrCpy $DirLocked ""
  StrCpy $0 $INSTDIR
  ${Do}
    ${GetParent} "$0" $0
    ${If} $0 == ""
      ${ExitDo}
    ${EndIf}
    Push $0
    Call ReadMarker
    ; 上層的標記檔存在卻打不開：可能是一份還在的安裝，不能確定這裡不在它裡面
    ${If} $MkUnreadable == "1"
      StrCpy $DirLocked $0
      ${ExitDo}
    ${EndIf}
    ${If} $MkValid == "1"
      ${If} $MkState == "installed"
        StrCpy $DirInside $0
      ${Else}
        StrLen $1 "$0\save"
        StrCpy $2 $INSTDIR $1
        ${If} $2 == "$0\save"
          StrCpy $2 $INSTDIR 1 $1
          ${If} $2 == ""
          ${OrIf} $2 == "\"
            StrCpy $DirInside $0
          ${EndIf}
        ${EndIf}
      ${EndIf}
      ${If} $DirInside != ""
        StrCpy $DirInsideState $MkState
        ${ExitDo}
      ${EndIf}
    ${EndIf}
  ${Loop}
  Push $INSTDIR
  Call PathState
  Pop $0
  ${If} $0 == "file"
    StrCpy $DirClass "file"
  ${ElseIf} $DirInside != ""
    StrCpy $DirClass "inside"
  ${ElseIf} $DirLocked != ""
    StrCpy $DirClass "locked"
  ${ElseIf} $0 == "none"
    StrCpy $DirClass "empty"
  ${ElseIf} $0 == "other"
    ; 存在但讀不到屬性：不能當成空的
    StrCpy $DirClass "foreign"
    StrCpy $DirUnlistable "1"
  ${Else}
    Push $INSTDIR
    Call ReadMarker
    ${If} $MkValid == "1"
      StrCpy $DirClass "ours"
    ${ElseIf} $MkUnreadable == "1"
      ; 標記檔存在卻打不開（遊戲或防毒軟體正開著它、權限）：和解除安裝端一樣當成那份安裝還在，不裝、也不改裝到子資料夾
      StrCpy $DirClass "locked"
      StrCpy $DirLocked $INSTDIR
    ${Else}
      ; 判斷空資料夾時忽略 desktop.ini、Thumbs.db 與 CheckInstDir 的試寫暫存檔；列不出內容就不能當成空的
      StrCpy $DirClass "empty"
      ClearErrors
      FindFirst $1 $2 "$INSTDIR\*.*"
      ${If} ${Errors}
        StrCpy $DirClass "foreign"
        StrCpy $DirUnlistable "1"
      ${EndIf}
      ${DoWhile} $2 != ""
        ${If} $2 != "."
        ${AndIf} $2 != ".."
        ${AndIf} $2 != "desktop.ini"
        ${AndIf} $2 != "Thumbs.db"
        ${AndIf} $2 != "${WRITE_TEST}"
          StrCpy $DirClass "foreign"
          ${ExitDo}
        ${EndIf}
        FindNext $1 $2
      ${Loop}
      FindClose $1
      ${If} $DirClass == "foreign"
      ${AndIf} $DirUnlistable != "1"
      ${AndIfNot} ${FileExists} "$INSTDIR\${MARKER_NAME}"
        Push "$INSTDIR\${EXE_NAME}"
        Call PathState
        Pop $0
        ${If} $0 == "file"
          Push "$INSTDIR\${EXE_NAME}"
          Call IsOurLauncher
          Pop $0
          ${If} $0 == "1"
            StrCpy $DirClass "portable"
          ${EndIf}
        ${EndIf}
      ${EndIf}
    ${EndIf}
  ${EndIf}
  Pop $2
  Pop $1
  Pop $0
FunctionEnd

; 這個位置上已經裝著的版本（ClassifyInstDir 之後 Call PrevInstall）：$PrevVer＝空字串（不是本程式已安裝的位置：空的、
; 免安裝版、解除過只留下存檔的 state=removed、別人的）；「新增或移除程式」的項目指向這裡時是它的 DisplayVersion
; （例如 RC1、RC2）；項目不在、指向別處或沒有版本時是「舊版」。只讀現有的標記檔與登錄，不另外記任何東西
Var PrevVer
Function PrevInstall
  Push $0
  StrCpy $PrevVer ""
  ${If} $DirClass == "ours"
  ${AndIf} $MkState == "installed"
    StrCpy $PrevVer "舊版"
    Call RegisteredHere
    Pop $0
    ${If} $0 == "1"
      ReadRegStr $0 HKCU "${UNINSTALL_KEY}" "DisplayVersion"
      ${If} $0 != ""
        StrCpy $PrevVer $0
      ${EndIf}
    ${EndIf}
  ${EndIf}
  Pop $0
FunctionEnd

; 磁碟根目錄（C:\、D:\、\\伺服器\共用資料夾）與 Windows 資料夾（本身與底下，不分大小寫）
Var CheckIsRoot   ; "1"：CheckInstDirRoot 擋下的是磁碟根目錄（不是 Windows 資料夾）
Function CheckInstDirRoot
  StrCpy $CheckKind "ok"
  StrCpy $CheckMsg ""
  StrCpy $CheckIsRoot "0"
  ${GetRoot} "$INSTDIR" $R0
  StrLen $R1 $WINDIR
  StrCpy $R2 $INSTDIR $R1
  StrCpy $R3 $INSTDIR 1 $R1
  ${If} $INSTDIR == $R0
  ${OrIf} $INSTDIR == "$R0\"
    StrCpy $CheckKind "block"
    StrCpy $CheckIsRoot "1"
    StrCpy $CheckCode ${EXIT_BAD_DIR}
    StrCpy $CheckMsg "不能直接裝在磁碟的根目錄（$INSTDIR）。$\r$\n$\r$\n請在後面加上資料夾名稱，例如：$\r$\n$R0\${APP_ID}"
  ${ElseIf} $R2 == $WINDIR
  ${AndIf} $R3 == ""
    StrCpy $CheckKind "block"
  ${ElseIf} $R2 == $WINDIR
  ${AndIf} $R3 == "\"
    StrCpy $CheckKind "block"
  ${EndIf}
  ${If} $CheckKind == "block"
  ${AndIf} $CheckMsg == ""
    StrCpy $CheckCode ${EXIT_BAD_DIR}
    StrCpy $CheckMsg "不能裝在 Windows 系統資料夾（$WINDIR）裡，請改選其他位置。"
  ${EndIf}
FunctionEnd

; $DirClass == "locked" 的說明 → $CheckMsg（CheckInstDir 與安裝段開頭的再分類共用）
Function LockedMsg
  StrCpy $CheckMsg "這個位置的安裝標記檔打不開：$\r$\n$DirLocked\${MARKER_NAME}$\r$\n$\r$\n可能是遊戲正開著、防毒軟體正在掃描它，或其他程式正在使用這個資料夾，無法確認那裡的安裝狀態，所以先不安裝。$\r$\n請關掉遊戲與其他正在使用它的程式（以及防毒軟體的掃描）後再試一次。"
FunctionEnd

; 安裝位置的完整檢查（不寫任何檔就能判斷的先做，最後才試寫）：根目錄／Windows 資料夾 → 是檔案 → 在另一個安裝裡面 →
; 標記檔打不開（擋下，結束碼 ${EXIT_WRITE_FAILED}）→
; 不是本程式的資料夾（不裝進去，也不在裡面試寫）→ 試寫
Function CheckInstDir
  StrCpy $CheckMsg ""
  Call CheckInstDirRoot
  ${If} $CheckKind != "ok"
    Return
  ${EndIf}

  Call ClassifyInstDir
  ${If} $DirClass == "file"
    StrCpy $CheckKind "block"
    StrCpy $CheckCode ${EXIT_BAD_DIR}
    StrCpy $CheckMsg "這個路徑是一個檔案，不是資料夾：$\r$\n$INSTDIR$\r$\n$\r$\n請改選資料夾，或在路徑後面加上新的資料夾名稱。"
    Return
  ${ElseIf} $DirClass == "inside"
    StrCpy $CheckKind "block"
    StrCpy $CheckCode ${EXIT_NOT_OURS_DIR}
    ${If} $DirInsideState == "installed"
      StrCpy $0 "本程式安裝在那裡（標記檔 $DirInside\${MARKER_NAME}，狀態 installed）"
    ${Else}
      StrCpy $0 "那裡解除安裝時保留了存檔（標記檔 $DirInside\${MARKER_NAME}，狀態 $DirInsideState），這裡是它的 save 資料夾"
    ${EndIf}
    StrCpy $CheckMsg "這個位置在另一個安裝的資料夾裡面：$\r$\n$DirInside$\r$\n$0$\r$\n$\r$\n請另選一個不在它裡面的資料夾，例如預設的：$\r$\n$LOCALAPPDATA\Programs\${APP_ID}"
    Return
  ${ElseIf} $DirClass == "locked"
    StrCpy $CheckKind "block"
    StrCpy $CheckCode ${EXIT_WRITE_FAILED}
    Call LockedMsg
    Return
  ${ElseIf} $DirClass == "foreign"
    ; 已有其他檔案、不是本程式的資料夾：不裝進去（圖形介面由 DirLeave 改裝到裡面的 ${APP_ID} 資料夾）
    StrCpy $CheckKind "foreign"
    StrCpy $CheckCode ${EXIT_NOT_OURS_DIR}
    StrCpy $CheckMsg "這個資料夾已有其他檔案，不是本程式的安裝位置：$\r$\n$INSTDIR$\r$\n$\r$\n不會裝進去。請改用空資料夾，或在路徑後面加上新的資料夾名稱。"
    Return
  ${EndIf}

  ; 實際試寫。建立之前先逐層記下哪些原本不存在：$R1＝往上第一層已存在的（用屬性判斷，存在但讀不到內容也算存在），
  ; $INSTDIR 到 $R1 之間（不含 $R1）就是這次會新建的；寫一個暫存檔再刪掉，最後只移除這些新建、仍是空的資料夾（RMDir 不遞迴）
  StrCpy $R1 $INSTDIR
  ${Do}
    Push $R1
    Call PathState
    Pop $R2
    ${If} $R2 != "none"
      ${ExitDo}
    ${EndIf}
    ${GetParent} "$R1" $R1
    ${If} $R1 == ""
      ${ExitDo}
    ${EndIf}
  ${Loop}
  StrCpy $R4 "ok"
  ClearErrors
  CreateDirectory "$INSTDIR"
  ${If} ${Errors}
    StrCpy $R4 "fail"
  ${Else}
    ClearErrors
    FileOpen $R2 "$INSTDIR\${WRITE_TEST}" w
    ${If} ${Errors}
      StrCpy $R4 "fail"
    ${Else}
      FileClose $R2
      Delete "$INSTDIR\${WRITE_TEST}"
      ${If} ${FileExists} "$INSTDIR\${WRITE_TEST}"
        StrCpy $R4 "left"
      ${EndIf}
    ${EndIf}
  ${EndIf}
  StrCpy $R3 $INSTDIR
  ${Do}
    ${If} $R3 == $R1
    ${OrIf} $R3 == ""
      ${ExitDo}
    ${EndIf}
    RMDir "$R3"
    ${GetParent} "$R3" $R3
  ${Loop}
  ${If} $R4 == "left"
    ; 試寫的暫存檔刪不掉（被別的程式開著）：不留下它繼續裝
    StrCpy $CheckKind "block"
    StrCpy $CheckCode ${EXIT_BAD_DIR}
    StrCpy $CheckMsg "試寫的暫存檔刪不掉（可能被別的程式開著）：$\r$\n$INSTDIR\${WRITE_TEST}$\r$\n$\r$\n請關掉正在使用它的程式、刪掉這個檔，或改選其他位置。"
    Return
  ${EndIf}
  ${If} $R4 == "fail"
    StrCpy $CheckKind "block"
    StrCpy $CheckCode ${EXIT_BAD_DIR}
    StrCpy $CheckMsg "目前的使用者無法寫入這個位置：$\r$\n$INSTDIR$\r$\n$\r$\n遊戲的存檔、ROM 複本與美術覆蓋圖會放在安裝資料夾裡的 save 資料夾，HD 美術包也放在遊戲旁邊，所以安裝位置必須可以寫入。這個安裝程式不使用系統管理員權限，像 C:\Program Files 這類位置寫不進去。$\r$\n$\r$\n請改選自己的資料夾，例如預設的：$\r$\n$LOCALAPPDATA\Programs\${APP_ID}"
    Return
  ${EndIf}
FunctionEnd

; ---- 選遊戲 ROM 與 IPS ----
; 與啟動器的約定：ROM 複製成 save\setup\rom.sfc 或 rom.smc（依原副檔名），IPS 複製成 save\setup\dbz102.ips；
; 啟動器在 save\cache\rom\canonical.sfc 不存在時先讀這裡、走它原本的驗證，成功後刪掉複本，失敗則刪掉、說明原因、改開檔案對話框。
; 安裝程式只做輕量檢查（檔案存在、大小不是 0、副檔名），不算雜湊、不修補，原檔不動。
; 已有 canonical.sfc 時啟動器不讀預放檔，現行也沒有換 ROM 的方法（main.rs 只在 canonical.sfc 不存在時設定 ROM），
; 所以那時欄位停用、只顯示說明；靜默安裝帶的 /ROM、/IPS 會略過並寫進紀錄。
Var RomPath
Var IpsPath
Var RomExt
Var RomEdit
Var IpsEdit
Var RomBrowse
Var IpsBrowse

; Push 路徑；Call CleanPath；Pop 得到去掉前後引號（Windows 11「複製路徑」會帶引號）並正規化的結果
Function CleanPath
  Exch $0
  Push $1
  StrCpy $1 $0 1
  ${If} $1 == "$\""
    StrCpy $0 $0 "" 1
  ${EndIf}
  StrCpy $1 $0 1 -1
  ${If} $1 == "$\""
    StrCpy $0 $0 -1
  ${EndIf}
  ${If} $0 != ""
    Push $0
    Call NormPath
    Pop $0
  ${EndIf}
  Pop $1
  Exch $0
FunctionEnd

; 檔案存在（是檔案）且大小不是 0：$R0＝路徑；回傳 $R0＝"ok"／"missing"／"empty"
Function CheckPickedFile
  Push $R0
  Call PathState
  Pop $R1
  ${If} $R1 != "file"
    StrCpy $R0 "missing"
    Return
  ${EndIf}
  ClearErrors
  FileOpen $R1 "$R0" r
  ${If} ${Errors}
    StrCpy $R0 "missing"
    Return
  ${EndIf}
  FileSeek $R1 0 END $R2
  FileClose $R1
  ${If} $R2 == 0
    StrCpy $R0 "empty"
  ${Else}
    StrCpy $R0 "ok"
  ${EndIf}
FunctionEnd

; $RomPath／$IpsPath 的輕量檢查（圖形介面的 RomPageLeave 與靜默安裝的 .onInit 呼叫同一份）；
; 會把兩個路徑整理成 CleanPath 的結果，並設定 $RomExt
Function CheckRomInputs
  StrCpy $CheckKind "ok"
  StrCpy $CheckCode ${EXIT_BAD_ROM}
  StrCpy $CheckMsg ""
  Push $RomPath
  Call CleanPath
  Pop $RomPath
  Push $IpsPath
  Call CleanPath
  Pop $IpsPath

  ${If} ${FileExists} "$INSTDIR\save\cache\rom\canonical.sfc"
    ${If} $RomPath != ""
    ${OrIf} $IpsPath != ""
      Push "這個位置已經設定過 ROM，略過 /ROM、/IPS：$RomPath $IpsPath"
      Call Log
    ${EndIf}
    StrCpy $RomPath ""
    StrCpy $IpsPath ""
    Return
  ${EndIf}

  ${If} $RomPath == ""
    ${If} $IpsPath != ""
      StrCpy $CheckKind "block"
      StrCpy $CheckMsg "選了 IPS 但沒有選 ROM：請先選遊戲 ROM，或清空 IPS 欄位。"
    ${EndIf}
    Return
  ${EndIf}

  ${GetFileExt} "$RomPath" $R3
  ${If} $R3 == "sfc"
    StrCpy $RomExt "sfc"
  ${ElseIf} $R3 == "smc"
    StrCpy $RomExt "smc"
  ${Else}
    StrCpy $CheckKind "block"
    StrCpy $CheckMsg "遊戲 ROM 要是 .sfc 或 .smc 檔：$\r$\n$RomPath"
    Return
  ${EndIf}
  StrCpy $R0 $RomPath
  Call CheckPickedFile
  ${If} $R0 == "missing"
    StrCpy $CheckKind "block"
    StrCpy $CheckMsg "找不到這個 ROM 檔，或無法讀取：$\r$\n$RomPath"
    Return
  ${ElseIf} $R0 == "empty"
    StrCpy $CheckKind "block"
    StrCpy $CheckMsg "這個 ROM 檔是空的（0 位元組）：$\r$\n$RomPath"
    Return
  ${EndIf}

  ${If} $IpsPath != ""
    ${GetFileExt} "$IpsPath" $R3
    ${If} $R3 != "ips"
      StrCpy $CheckKind "block"
      StrCpy $CheckMsg "IPS 要是 .ips 檔（dbz102.ips）：$\r$\n$IpsPath"
      Return
    ${EndIf}
    StrCpy $R0 $IpsPath
    Call CheckPickedFile
    ${If} $R0 == "missing"
      StrCpy $CheckKind "block"
      StrCpy $CheckMsg "找不到這個 IPS 檔，或無法讀取：$\r$\n$IpsPath"
    ${ElseIf} $R0 == "empty"
      StrCpy $CheckKind "block"
      StrCpy $CheckMsg "這個 IPS 檔是空的（0 位元組）：$\r$\n$IpsPath"
    ${EndIf}
  ${EndIf}
FunctionEnd

; Call TrimInstDir：$INSTDIR 不是磁碟根目錄時去掉結尾的一個 \（InstallDir 以 \ 結尾，見上方 InstallDir 的說明），
; 登錄的 InstallLocation、UninstallString 與捷徑目標才不會出現連續兩個反斜線
Function TrimInstDir
  Push $0
  StrCpy $0 $INSTDIR 1 -1
  ${If} $0 == "\"
    ${GetRoot} "$INSTDIR" $0
    ${If} "$0\" != $INSTDIR
      StrCpy $INSTDIR $INSTDIR -1
    ${EndIf}
  ${EndIf}
  Pop $0
FunctionEnd

; Call RequestedDir：原始命令列有沒有 /D=（$ReqDirGiven＝"1"）與它的值（$ReqDir，去掉結尾空白；帶了空值就是空字串）。
; $CMDLINE 已拿掉 /D=，所以讀 GetCommandLineW；/D= 一定是最後一個參數、不加引號（NSIS 的規定），取最後一個 " /D=" 之後的全部
Var ReqDirGiven
Var ReqDir
Function RequestedDir
  Push $0
  Push $1
  Push $2
  System::Call 'kernel32::GetCommandLineW() w .r0'
  StrCpy $ReqDirGiven "0"
  StrCpy $ReqDir ""
  StrLen $1 $0
  ${Do}
    IntOp $1 $1 - 1
    ${If} $1 < 0
      ${ExitDo}
    ${EndIf}
    StrCpy $2 $0 4 $1
    ${If} $2 S== " /D="
      IntOp $1 $1 + 4
      StrCpy $ReqDir $0 "" $1
      StrCpy $ReqDirGiven "1"
      ${ExitDo}
    ${EndIf}
  ${Loop}
  ${Do}
    StrCpy $2 $ReqDir 1 -1
    ${If} $2 != " "
    ${AndIf} $2 != "$\t"
      ${ExitDo}
    ${EndIf}
    StrCpy $ReqDir $ReqDir -1
  ${Loop}
  Pop $2
  Pop $1
  Pop $0
FunctionEnd

; 靜默安裝：在寫任何檔之前做完全部檢查；不合（含已有其他檔案的資料夾）就寫紀錄、設定結束碼並結束
Function .onInit
  ${GetParameters} $R9
  Push $R9
  Call NormParams
  Pop $R9
  Push $R9
  Push "/LOG="
  Call GetOpt
  Pop $LogPath
  ${If} $LogPath == ""
    StrCpy $LogPath "$TEMP\${APP_ID}-setup.log"
  ${EndIf}
  Push $R9
  Push "/ROM="
  Call GetOpt
  Pop $RomPath
  Push $R9
  Push "/IPS="
  Call GetOpt
  Pop $IpsPath

  ; 歡迎頁的第一段：上次的安裝位置（InstallDirRegKey 帶出的 $INSTDIR）已經裝著本程式時，說這是原地升級。
  ; 只看不改：$INSTDIR 照原樣還回去（選位置頁顯示的值與結尾的 \ 都不變）
  StrCpy $R6 $INSTDIR
  Call TrimInstDir
  Call ClassifyInstDir
  Call PrevInstall
  StrCpy $INSTDIR $R6
  ${If} $PrevVer S== "${DISPLAY_VERSION}"
    StrCpy $WelcomeLead "偵測到已安裝的 ${DISPLAY_VERSION}，這會在原位置重新安裝，存檔與 HD 美術包都會保留。"
  ${ElseIf} $PrevVer != ""
    StrCpy $WelcomeLead "偵測到已安裝的 $PrevVer，這會原地升級為 ${DISPLAY_VERSION}，存檔與 HD 美術包都會保留。"
  ${Else}
    StrCpy $WelcomeLead "這會安裝 ${DISPLAY_VERSION}，下一步可以選擇要裝在哪個資料夾。"
  ${EndIf}

  ${If} ${Silent}
    ; NSIS 只採用可用的 /D=：空值、磁碟根目錄、是檔案、不是完整路徑時，它會默默換回預設位置照裝。
    ; 帶了 /D= 卻是空值，或不是 $INSTDIR 那個位置，就當成不能用的位置中止，不裝到沒指定的地方
    Call RequestedDir
    ${If} $ReqDirGiven == "1"
    ${AndIf} $ReqDir == ""
      Push "中止（結束碼 ${EXIT_BAD_DIR}）：帶了 /D= 但沒有給資料夾"
      Call Log
      SetErrorLevel ${EXIT_BAD_DIR}
      Quit
    ${ElseIf} $ReqDirGiven == "1"
      Push $ReqDir
      Push $INSTDIR
      Call SamePath
      Pop $R7
      ${If} $R7 != "1"
        Push "中止（結束碼 ${EXIT_BAD_DIR}）：不能用這個安裝位置：$ReqDir（磁碟根目錄、是檔案，或不是完整的資料夾路徑）"
        Call Log
        SetErrorLevel ${EXIT_BAD_DIR}
        Quit
      ${EndIf}
    ${EndIf}
    Call TrimInstDir
    Push "靜默安裝：$INSTDIR"
    Call Log
    Call CheckInstDir
    ${If} $CheckKind != "ok"
      Push "中止（結束碼 $CheckCode）：$CheckMsg"
      Call Log
      SetErrorLevel $CheckCode
      Quit
    ${EndIf}
    Call CheckRomInputs
    ${If} $CheckKind != "ok"
      Push "中止（結束碼 $CheckCode）：$CheckMsg"
      Call Log
      SetErrorLevel $CheckCode
      Quit
    ${EndIf}
  ${EndIf}
FunctionEnd

; 選安裝位置頁按「下一步」：頁面的 leave 函式呼叫 Abort 會留在本頁（NSIS 文件 4.5.3）；
; .onVerifyInstDir 每改一個字就跑，不適合試寫與跳訊息（4.7.2.1.x）
Function DirLeave
  Call TrimInstDir
  ; 根目錄放在 GetInstDirError 之前：NSIS 預設 AllowRootDirInstall false，根目錄在那裡只會得到「路徑無效」。
  ; 選到磁碟根目錄（例如按「瀏覽」選了 D:\）：比照別人的資料夾，改到 <根>\${APP_ID} 並告知，再照常分類
  Call CheckInstDirRoot
  ${If} $CheckKind != "ok"
  ${AndIf} $CheckIsRoot == "1"
    ${GetRoot} "$INSTDIR" $R0
    StrCpy $INSTDIR "$R0\${APP_ID}"
    ${NSD_SetText} $mui.DirectoryPage.Directory $INSTDIR
    MessageBox MB_OK|MB_ICONINFORMATION "不能直接裝在磁碟的根目錄（$R0\），會改裝到：$\r$\n$INSTDIR"
    Call CheckInstDirRoot
  ${EndIf}
  ${If} $CheckKind != "ok"
    MessageBox MB_OK|MB_ICONEXCLAMATION "$CheckMsg"
    Abort
  ${EndIf}
  GetInstDirError $R0
  ${If} $R0 == 1
    MessageBox MB_OK|MB_ICONEXCLAMATION "這不是有效的資料夾路徑：$\r$\n$INSTDIR$\r$\n$\r$\n請重新輸入，或按「瀏覽」挑一個資料夾。"
    Abort
  ${ElseIf} $R0 == 2
    MessageBox MB_OK|MB_ICONEXCLAMATION "這個位置所在的磁碟空間不夠，請改選其他磁碟上的資料夾。"
    Abort
  ${EndIf}

  Call CheckInstDir
  ${If} $CheckKind == "foreign"
    ; 不是本程式的資料夾：不裝進去，改裝到它裡面的 ${APP_ID} 資料夾並告知（對子資料夾重新分類：本程式的、空的、
    ; 免安裝版的才裝）；子資料夾也不能用就擋下，告訴使用者可以直接選哪個資料夾
    StrCpy $R8 $INSTDIR
    StrCpy $INSTDIR "$R8\${APP_ID}"
    Call CheckInstDir
    ${If} $CheckKind == "ok"
      ; 欄位也改成新位置：NSIS 離開本頁後會再從欄位讀一次 $INSTDIR，只改變數會被蓋回原本選的資料夾
      ${NSD_SetText} $mui.DirectoryPage.Directory $INSTDIR
      StrCpy $R7 ""
      ${If} ${FileExists} "$R8\save\${SYNC_MARKER}"
        StrCpy $R7 "$\r$\n$\r$\n這個資料夾裡有先前的存檔（$R8\save\），裝到子資料夾後遊戲不會讀到它。"
      ${EndIf}
      MessageBox MB_OK|MB_ICONINFORMATION "這個資料夾已有其他檔案：$\r$\n$R8$\r$\n$\r$\n為了不動到裡面的檔案，會改裝到它裡面的新資料夾：$\r$\n$INSTDIR$R7"
    ${Else}
      ${If} $CheckKind == "foreign"
      ${AndIf} $DirUnlistable == "1"
        StrCpy $CheckMsg "這個資料夾已有其他檔案：$\r$\n$R8$\r$\n$\r$\n裡面的 ${APP_ID} 資料夾讀不到內容（可能沒有權限），無法確認能不能裝，所以兩個都不能裝。$\r$\n$\r$\n可以直接選預設的位置：$\r$\n$LOCALAPPDATA\Programs\${APP_ID}$\r$\n或任何一個空資料夾。"
      ${ElseIf} $CheckKind == "foreign"
        StrCpy $CheckMsg "這個資料夾已有其他檔案：$\r$\n$R8$\r$\n$\r$\n裡面的 ${APP_ID} 資料夾也有別的檔案，不是本程式的安裝位置，所以兩個都不能裝。$\r$\n$\r$\n可以直接選預設的位置：$\r$\n$LOCALAPPDATA\Programs\${APP_ID}$\r$\n或任何一個空資料夾。"
      ${EndIf}
      StrCpy $INSTDIR $R8
      MessageBox MB_OK|MB_ICONEXCLAMATION "$CheckMsg"
      Abort
    ${EndIf}
  ${ElseIf} $CheckKind != "ok"
    MessageBox MB_OK|MB_ICONEXCLAMATION "$CheckMsg"
    Abort
  ${EndIf}

  ; 重裝到和上次不同的位置、上次的位置有 save\：存檔與 ROM 設定不會自動搬過來，提醒後讓使用者決定（不自動搬）
  ReadRegStr $R5 HKCU "${UNINSTALL_KEY}" "InstallLocation"
  ${If} $R5 != ""
    Call RegisteredHere
    Pop $R6
    Push "$R5\save"
    Call PathState
    Pop $R7
    ${If} $R6 != "1"
    ${AndIf} $R7 == "dir"
      ${IfNot} ${Cmd} `MessageBox MB_YESNO|MB_ICONINFORMATION "上次裝在：$\r$\n$R5$\r$\n$\r$\n存檔與 ROM 設定（那裡的 save 資料夾）會留在舊位置，不會自動搬到新位置。要沿用的話，可以改回舊位置安裝，或安裝後自己把整個 save 資料夾搬過來。$\r$\n$\r$\n舊位置的程式也不會被移除，只是「新增或移除程式」會改成指向新位置；不需要舊的那份時，可以執行舊位置裡的 ${UNINSTALLER} 移除它（存檔預設保留）。$\r$\n$\r$\n仍要裝到新位置嗎？" IDYES`
        Abort
      ${EndIf}
    ${EndIf}
  ${EndIf}
FunctionEnd

Function RomPageShow
  !insertmacro MUI_HEADER_TEXT "選擇遊戲 ROM 與 IPS" "可以略過：第一次開啟遊戲時，啟動器會再請你選擇。"
  nsDialogs::Create 1018
  Pop $R0
  ${If} $R0 == error
    Abort
  ${EndIf}

  ${NSD_CreateLabel} 0u 0u 100% 26u "安裝程式不含遊戲 ROM。在這裡選你自己的檔案，裝好後第一次開啟遊戲就不用再選；兩欄都留空就是略過，第一次開啟時啟動器會再問。"
  Pop $R0
  ${NSD_CreateLabel} 0u 30u 100% 10u "遊戲 ROM（.sfc／.smc）：可用日版 Rev 1 ROM，或 Klepto v1.02 英文版 ROM。"
  Pop $R0
  ${NSD_CreateText} 0u 42u 238u 13u "$RomPath"
  Pop $RomEdit
  ${NSD_CreateButton} 244u 41u 56u 15u "瀏覽(&R)..."
  Pop $RomBrowse
  ${NSD_OnClick} $RomBrowse RomBrowseClick
  ${NSD_CreateLabel} 0u 64u 100% 10u "IPS（dbz102.ips）：日版 ROM 需要；英文版 ROM 請留空。"
  Pop $R0
  ${NSD_CreateText} 0u 76u 238u 13u "$IpsPath"
  Pop $IpsEdit
  ${NSD_CreateButton} 244u 75u 56u 15u "瀏覽(&W)..."
  Pop $IpsBrowse
  ${NSD_OnClick} $IpsBrowse IpsBrowseClick

  ${If} ${FileExists} "$INSTDIR\save\cache\rom\canonical.sfc"
    StrCpy $RomPath ""
    StrCpy $IpsPath ""
    ${NSD_SetText} $RomEdit ""
    ${NSD_SetText} $IpsEdit ""
    EnableWindow $RomEdit 0
    EnableWindow $RomBrowse 0
    EnableWindow $IpsEdit 0
    EnableWindow $IpsBrowse 0
    ${NSD_CreateLabel} 0u 100u 100% 30u "這個位置已經設定過 ROM，可以直接繼續。$\r$\n已設定的 ROM 不會被這次安裝更動，這裡也不能更換。"
  ${Else}
    ; 自我檢查的說明，每句對到啟動器做的事：只複製、原檔不動（CopyPreset）；第一次開啟時檢查（preset.rs run：
    ; canonical.sfc 不存在時）；ROM 用 CRC32 認版本、日版套 IPS 後再核對結果（crates/ssd-rom normalize：Unknown、
    ; PatchResultMismatch）；不對就說明原因、刪掉複本、改開選檔對話框（preset.rs：report、clear，main.rs set_up_rom）
    ${NSD_CreateLabel} 0u 100u 100% 30u "選好的檔會複製到安裝資料夾的 save\setup\，原檔不會被改動。$\r$\n第一次開啟遊戲時，程式會自己檢查 ROM 是不是可用的版本，$\r$\n日版 ROM 套上 IPS 的結果也會核對；不對的話會說明原因，$\r$\n並刪掉那份複本、請你重新選擇。"
  ${EndIf}
  Pop $R0
  Call SkinPage
  nsDialogs::Show
FunctionEnd

Function RomBrowseClick
  Pop $R0
  ${NSD_GetText} $RomEdit $R1
  nsDialogs::SelectFileDialog open "$R1" "SNES ROM (*.sfc;*.smc)|*.sfc;*.smc|所有檔案 (*.*)|*.*"
  Pop $R1
  ${If} $R1 != ""
    ${NSD_SetText} $RomEdit "$R1"
  ${EndIf}
FunctionEnd

Function IpsBrowseClick
  Pop $R0
  ${NSD_GetText} $IpsEdit $R1
  nsDialogs::SelectFileDialog open "$R1" "IPS 修補檔 (*.ips)|*.ips|所有檔案 (*.*)|*.*"
  Pop $R1
  ${If} $R1 != ""
    ${NSD_SetText} $IpsEdit "$R1"
  ${EndIf}
FunctionEnd

; ---- HD 高畫質美術包 ----
; 對照圖：ui/hd/plain_<dpi>.bmp、art_<dpi>.bmp（ui/make_hd_compare.py，遊戲實際截圖：story-1 重播第 8700 格，同一格
; 沒有美術包與有美術包各畫一次，裁同一範圍：悟空全身與周圍的天空和雲）。
; 說明的每一句對到啟動器（launcher/src/hd_offer.rs）：上游 JackRushante 的作品、CC BY-NC-SA 4.0；約 462 MB
; （HD_PACK_ZIP_BYTES 484,121,119）；下載中可取消；大小與 SHA-256 和上游原檔相同才放到 exe 旁；上游拿不到時改從備援
; 網址下載（啟動器的 MIRROR_URL）；不下載也能玩、畫面用原版像素；之後可從開始功能表「下載 HD 美術包」（--hd-pack）。
Var HdWanted   ; 1 = 完成時跑 --hd-pack（FinishRun／FinishLeave）
Var HdHave     ; 安裝位置已經有的美術包（HdFind），空字串 = 沒有
Var HdBox
Var HdPic
Var HdPic2

; 安裝位置（啟動器 exe 旁）有沒有啟動器會用的美術包，規則照 launcher/src/main.rs hd_pack_in：hd-pack.zip 這個檔、
; hd-pack 資料夾裡有 hd-pack.toml，或檔名含 hd-pack、以 .zip 結尾的檔（不分大小寫）。找到的名字放 $HdHave
Function HdFind
  StrCpy $HdHave ""
  ${If} ${FileExists} "$INSTDIR\hd-pack.zip"
  ${AndIfNot} ${FileExists} "$INSTDIR\hd-pack.zip\*.*"
    StrCpy $HdHave "hd-pack.zip"
    Return
  ${EndIf}
  ${If} ${FileExists} "$INSTDIR\hd-pack\hd-pack.toml"
  ${AndIfNot} ${FileExists} "$INSTDIR\hd-pack\hd-pack.toml\*.*"
    StrCpy $HdHave "hd-pack"
    Return
  ${EndIf}
  ClearErrors
  FindFirst $R0 $R1 "$INSTDIR\*hd-pack*.zip"
  ${DoWhile} $R1 != ""
    StrCpy $R2 $R1 "" -4
    ${If} $R2 == ".zip"
    ${AndIfNot} ${FileExists} "$INSTDIR\$R1\*.*"
      StrCpy $HdHave $R1
    ${EndIf}
    FindNext $R0 $R1
  ${Loop}
  FindClose $R0
FunctionEnd

Function HdPageShow
  ${If} $HdPic P<> 0
    ${NSD_FreeImage} $HdPic
    StrCpy $HdPic 0
  ${EndIf}
  ${If} $HdPic2 P<> 0
    ${NSD_FreeImage} $HdPic2
    StrCpy $HdPic2 0
  ${EndIf}
  Call HdFind
  !insertmacro MUI_HEADER_TEXT "HD 高畫質美術包" "想要高畫質的畫面，需要 HD 美術包；$\r$\n沒有也能玩，畫面是原版的像素圖。"
  nsDialogs::Create 1018
  Pop $R0
  ${If} $R0 == error
    Abort
  ${EndIf}
  ; 頁面大小 $R4×$R5
  System::Call '*(i,i,i,i) p .R3'
  System::Call 'user32::GetClientRect(p R0, p R3) i'
  System::Call '*$R3(i, i, i .R4, i .R5)'
  System::Free $R3
  ; 對照圖兩張（原版在上、HD 在下，同一格同一範圍的完整裁切），疊在左欄、相隔 $R3。尺寸 $R6 照系統 DPI $R2 選
  ; （192／144／120／96 各一組，各自從截圖縮好，不讓 Windows 拉伸），兩張疊起來放不進頁高時用小一號
  System::Call 'user32::GetDC(p 0) p .R1'
  System::Call 'gdi32::GetDeviceCaps(p R1, i 90) i .R2'
  System::Call 'user32::ReleaseDC(p 0, p R1) i'
  IntOp $R3 $R2 * 6
  IntOp $R3 $R3 / 96
  StrCpy $R6 96
  ${If} $R2 >= 192
    IntOp $R1 400 + $R3
    ${If} $R1 <= $R5
      StrCpy $R6 192
    ${EndIf}
  ${EndIf}
  ${If} $R6 = 96
  ${AndIf} $R2 >= 144
    IntOp $R1 300 + $R3
    ${If} $R1 <= $R5
      StrCpy $R6 144
    ${EndIf}
  ${EndIf}
  ${If} $R6 = 96
  ${AndIf} $R2 >= 120
    IntOp $R1 250 + $R3
    ${If} $R1 <= $R5
      StrCpy $R6 120
    ${EndIf}
  ${EndIf}
  File "/oname=$PLUGINSDIR\hd_plain_96.bmp" "ui\hd\plain_96.bmp"
  File "/oname=$PLUGINSDIR\hd_art_96.bmp" "ui\hd\art_96.bmp"
  File "/oname=$PLUGINSDIR\hd_plain_120.bmp" "ui\hd\plain_120.bmp"
  File "/oname=$PLUGINSDIR\hd_art_120.bmp" "ui\hd\art_120.bmp"
  File "/oname=$PLUGINSDIR\hd_plain_144.bmp" "ui\hd\plain_144.bmp"
  File "/oname=$PLUGINSDIR\hd_art_144.bmp" "ui\hd\art_144.bmp"
  File "/oname=$PLUGINSDIR\hd_plain_192.bmp" "ui\hd\plain_192.bmp"
  File "/oname=$PLUGINSDIR\hd_art_192.bmp" "ui\hd\art_192.bmp"
  ; 原版：左上，大小照圖本身 $R7×$R8
  ${NSD_CreateBitmap} 0 0 10 10 ""
  Pop $R1
  ${NSD_SetImage} $R1 "$PLUGINSDIR\hd_plain_$R6.bmp" $HdPic
  System::Call '*(i,i,i,i,i,i,p) p .R9'
  System::Call 'gdi32::GetObjectW(p $HdPic, i 32, p R9) i'
  System::Call '*$R9(i, i .R7, i .R8)'
  System::Free $R9
  System::Call 'user32::SetWindowPos(p R1, p 0, i 0, i 0, i R7, i R8, i 0x14) i'
  ; HD：在它下面
  ${NSD_CreateBitmap} 0 0 10 10 ""
  Pop $R1
  ${NSD_SetImage} $R1 "$PLUGINSDIR\hd_art_$R6.bmp" $HdPic2
  IntOp $R9 $R8 + $R3
  System::Call 'user32::SetWindowPos(p R1, p 0, i 0, i R9, i R7, i R8, i 0x14) i'
  IntOp $R8 $R9 + $R8                ; 左欄的高度
  ; 說明在圖的右邊（$R9 起、寬 $R1）
  IntOp $R3 $R2 * 10
  IntOp $R3 $R3 / 96
  IntOp $R9 $R7 + $R3
  IntOp $R1 $R4 - $R9
  ${If} $HdHave != ""
    ; 已經有美術包（重裝）：照實說，不再問
    ${NSD_CreateLabel} $R9 0 $R1 $R8 "這個安裝位置已經有 HD 美術包：$\r$\n$HdHave$\r$\n不用再下載。"
    Pop $R0
    StrCpy $HdBox 0
  ${Else}
    ; 手動換行，每行最多 19 個全形字（Windows 100% 時這一欄約 21 個：頁寬約 450 px 減去 180 px 的圖），斷在標點
    ${NSD_CreateLabel} $R9 0 $R1 $R8 "這一包是上游作者 JackRushante 的作品，$\r$\n授權 CC BY-NC-SA 4.0，$\r$\n不是本繁中版的內容，所以安裝檔裡沒有。$\r$\n$\r$\n約 462 MB，安裝完成後才下載，$\r$\n可隨時取消；現在不下載，$\r$\n之後也可以從開始功能表下載。$\r$\n$\r$\n下載後程式會核對檔案大小與 SHA-256，$\r$\n和上游原檔完全相同才使用；$\r$\n上游抓不到時會改從備援下載。"
    Pop $R0
    ; 說明照它需要的高度（SkinTextH，和 SkinPage 量的一樣），核取方塊緊接在下面。不先給左欄的高度再讓 SkinPage
    ; 加高：加高時下面的控制項都會往下移，左欄下面那張圖也會被移走
    Push $R0
    Push $R1
    Call SkinTextH
    System::Call 'user32::SetWindowPos(p R0, p 0, i R9, i 0, i R1, i $SkRH, i 0x14) i'
    IntOp $R3 $SkRH + $R3
    IntOp $R0 $R2 * 17
    IntOp $R0 $R0 / 96
    ${NSD_CreateCheckbox} $R9 $R3 $R1 $R0 "安裝完成後下載 HD 美術包${TEST_FINISH_BOX_PAD}"
    Pop $HdBox
    ${If} $HdWanted == 1
      ${NSD_Check} $HdBox
    ${EndIf}
  ${EndIf}
  Call SkinPage
  nsDialogs::Show
FunctionEnd

Function HdPageLeave
  StrCpy $HdWanted 0
  ${If} $HdBox P<> 0
    ${NSD_GetState} $HdBox $R0
    ${If} $R0 == ${BST_CHECKED}
      StrCpy $HdWanted 1
    ${EndIf}
  ${EndIf}
  ; 圖在頁面關掉前都還在用：下次顯示這一頁時才放掉（靜態控制項不會自己放掉它的圖）
FunctionEnd

Function RomPageLeave
  ${NSD_GetText} $RomEdit $RomPath
  ${NSD_GetText} $IpsEdit $IpsPath
  Call CheckRomInputs
  ; 整理後的路徑放回欄位
  ${NSD_SetText} $RomEdit $RomPath
  ${NSD_SetText} $IpsEdit $IpsPath
  ${If} $CheckKind != "ok"
    MessageBox MB_OK|MB_ICONEXCLAMATION "$CheckMsg"
    Abort
  ${EndIf}
FunctionEnd

; 完成頁「立即開啟遊戲」：HD 美術包頁勾了下載時，先藏起安裝程式、等 --hd-pack 跑完再開遊戲，兩個啟動器不同時下載同一個檔
Function FinishRun
  ${If} $HdWanted == 1
    HideWindow
    ExecWait '"$INSTDIR\${EXE_NAME}" --hd-pack'
  ${EndIf}
  Exec '"$INSTDIR\${EXE_NAME}"'
FunctionEnd

; 完成頁按「完成」：HD 美術包頁勾了下載、沒勾「立即開啟遊戲」時只跑 --hd-pack（兩個都勾時由 FinishRun 做）
Function FinishLeave
  ${NSD_GetState} $mui.FinishPage.Run $R0
  ${If} $HdWanted == 1
  ${AndIf} $R0 != ${BST_CHECKED}
    Exec '"$INSTDIR\${EXE_NAME}" --hd-pack'
  ${EndIf}
FunctionEnd

; Push 來源；Push 目的；Call CopyPreset：把選的檔複製到 save\setup\ 的約定檔名。來源就是目的本身（SamePath）時不刪不複製；
; 複製後去掉唯讀屬性（來源是唯讀檔時複本也會是唯讀），啟動器用完才刪得掉。Pop 得到 "copied"／"inplace"／"failed"
Function CopyPreset
  Exch $1
  Exch
  Exch $0
  Push $2
  Push $3
  Push $0
  Push $1
  Call SamePath
  Pop $2
  ${If} $2 == "1"
    StrCpy $0 "inplace"
  ${Else}
    Delete "$1"
    ClearErrors
    CopyFiles /SILENT /FILESONLY "$0" "$1"
    StrCpy $3 ""
    ${If} ${Errors}
      StrCpy $3 "error"
    ${EndIf}
    Push $1
    Call PathState
    Pop $2
    ${If} $3 == "error"
    ${OrIf} $2 != "file"
      StrCpy $0 "failed"
    ${Else}
      SetFileAttributes "$1" NORMAL
      StrCpy $0 "copied"
    ${EndIf}
  ${EndIf}
  Pop $3
  Pop $2
  Exch $0
  Exch
  Pop $1
FunctionEnd

Section "install"
  SetShellVarContext current
  ; 寫任何檔之前再分類一次（圖形介面已在 DirLeave、靜默已在 .onInit 檢查過；這裡防範之後 $INSTDIR 被改動）：
  ; 只裝進本程式的、空的、免安裝版的資料夾
  Call ClassifyInstDir
  ${If} $DirClass == "locked"
    ; 選完位置之後標記檔才被鎖住（遊戲或防毒在這期間開著它）：和 CheckInstDir 一樣回 14、用同一段說明
    Call LockedMsg
    Push "中止（結束碼 ${EXIT_WRITE_FAILED}）：$CheckMsg"
    Call Log
    MessageBox MB_OK|MB_ICONSTOP "$CheckMsg" /SD IDOK
    SetErrorLevel ${EXIT_WRITE_FAILED}
    Abort "安裝標記檔打不開，已中止"
  ${EndIf}
  ${If} $DirClass != "ours"
  ${AndIf} $DirClass != "empty"
  ${AndIf} $DirClass != "portable"
    Push "中止（結束碼 ${EXIT_NOT_OURS_DIR}）：$INSTDIR 不是本程式的資料夾（$DirClass）"
    Call Log
    MessageBox MB_OK|MB_ICONSTOP "這個資料夾不是本程式的安裝位置，安裝已中止：$\r$\n$INSTDIR" /SD IDOK
    SetErrorLevel ${EXIT_NOT_OURS_DIR}
    Abort "安裝位置不是本程式的資料夾，已中止"
  ${EndIf}
  ${If} $DirClass == "portable"
    Push "從免安裝版原地升級：$INSTDIR"
    Call Log
  ${EndIf}
  ; 這裡已經裝著本程式：說是升級還是同一版重裝（寫標記檔與登錄之前讀，讀到的是原本那一份的）。安裝畫面的詳細資訊
  ; 與紀錄都有這一行
  Call PrevInstall
  ${If} $PrevVer S== "${DISPLAY_VERSION}"
    Push "偵測到已安裝的 ${DISPLAY_VERSION}，在原位置重新安裝：$INSTDIR（存檔與 HD 美術包保留）"
    Call Log
  ${ElseIf} $PrevVer != ""
    Push "偵測到已安裝的 $PrevVer，原地升級為 ${DISPLAY_VERSION}：$INSTDIR（存檔與 HD 美術包保留）"
    Call Log
  ${EndIf}
  ; 第一個寫入動作：標記檔（state=installed）
  CreateDirectory "$INSTDIR"
  ${GetTime} "" "L" $0 $1 $2 $3 $4 $5 $6
  StrCpy $MkInstalled "$2-$1-$0 $4:$5:$6"
  Push "installed"
  Call WriteMarker
  Pop $0
  ${If} $0 != "ok"
    Push "中止（結束碼 ${EXIT_BAD_DIR}）：寫不進標記檔 $INSTDIR\${MARKER_NAME}"
    Call Log
    MessageBox MB_OK|MB_ICONSTOP "無法寫入安裝資料夾，安裝已中止：$\r$\n$INSTDIR" /SD IDOK
    SetErrorLevel ${EXIT_BAD_DIR}
    Abort "寫不進安裝資料夾，已中止"
  ${EndIf}
  SetOutPath "$INSTDIR"
  ClearErrors
  File "/oname=${EXE_NAME}" "${LAUNCHER_EXE}"
  ${If} ${Errors}
    StrCpy $0 "$INSTDIR\${EXE_NAME}"
  ${Else}
    ClearErrors
    WriteUninstaller "$INSTDIR\${UNINSTALLER}"
    ${If} ${Errors}
      StrCpy $0 "$INSTDIR\${UNINSTALLER}"
    ${Else}
      StrCpy $0 ""
    ${EndIf}
  ${EndIf}
  ${If} $0 != ""
    ; 寫不進去（例如遊戲正開著）：不寫捷徑與「新增或移除程式」的項目；標記檔已是 state=installed，關掉後再裝一次即可
    Push "中止（結束碼 ${EXIT_WRITE_FAILED}）：寫不進 $0"
    Call Log
    MessageBox MB_OK|MB_ICONSTOP "寫不進這個檔，安裝已中止：$\r$\n$0$\r$\n$\r$\n可能是遊戲正開著，或沒有權限。關掉遊戲後再執行一次安裝程式。" /SD IDOK
    SetErrorLevel ${EXIT_WRITE_FAILED}
    Abort "寫不進安裝資料夾，已中止"
  ${EndIf}
  ; 之前測試版的解除安裝程式叫 uninstall.exe：新的解除安裝程式寫好之後才刪（寫啟動器失敗中止時，舊的項目還指得到它）；
  ; 版本資源的 ProductName 是本程式的才刪，別的程式的不動
  ${If} ${FileExists} "$INSTDIR\uninstall.exe"
    Push "$INSTDIR\uninstall.exe"
    Push "ProductName"
    Call VersionString
    Pop $0
    ${If} $0 S== "${PRODUCT}"
      Delete "$INSTDIR\uninstall.exe"
      Push "刪除之前測試版的解除安裝程式：$INSTDIR\uninstall.exe"
    ${Else}
      Push "保留不是本程式的 uninstall.exe（ProductName：$0）"
    ${EndIf}
    Call Log
  ${EndIf}

  ; 開始功能表：一個資料夾放「遊戲」與「下載 HD 美術包」（--hd-pack 只下載美術包、不開始遊戲）；桌面只放遊戲。
  ; 先前的測試版把遊戲捷徑直接放在開始功能表，先移除那個位置的舊捷徑
  Delete "$SMPROGRAMS\${PRODUCT}.lnk"
  !insertmacro WRITE_SHORTCUTS

  ; 選了 ROM／IPS 時：複製到 save\setup\（約定檔名），原檔不動。這次選的取代先前預放、還沒被啟動器用掉的複本。
  ; 複製失敗只提示，不讓安裝失敗
  ${If} $RomPath != ""
    CreateDirectory "${SETUP_DIR}"
    Push $RomPath
    Push "${SETUP_DIR}\rom.$RomExt"
    Call CopyPreset
    Pop $R7
    Push "ROM（$R7）：$RomPath → ${SETUP_DIR}\rom.$RomExt"
    Call Log
    ${If} $R7 == "failed"
      MessageBox MB_OK|MB_ICONEXCLAMATION "無法把 ROM 複製到安裝資料夾（來源檔可能正在使用中，或磁碟空間不足）：$\r$\n$RomPath$\r$\n$\r$\n安裝會照常完成；第一次開啟遊戲時，啟動器會再請你選擇 ROM。" /SD IDOK
    ${ElseIf} $R7 == "copied"
      ; 換了新的 ROM：另一個副檔名的舊預放檔不再適用
      ${If} $RomExt == "sfc"
        Delete "${SETUP_DIR}\rom.smc"
      ${Else}
        Delete "${SETUP_DIR}\rom.sfc"
      ${EndIf}
    ${EndIf}

    ${If} $R7 == "failed"
      ; ROM 沒放好，IPS 也不放
    ${ElseIf} $IpsPath == ""
      ; 換了新的 ROM 卻沒選 IPS：先前預放的 IPS 不再對應，移除；ROM 是原地那份時不動
      ${If} $R7 == "copied"
        Delete "${SETUP_DIR}\${IPS_NAME}"
      ${EndIf}
    ${Else}
      Push $IpsPath
      Push "${SETUP_DIR}\${IPS_NAME}"
      Call CopyPreset
      Pop $R6
      Push "IPS（$R6）：$IpsPath → ${SETUP_DIR}\${IPS_NAME}"
      Call Log
      ${If} $R6 == "failed"
        MessageBox MB_OK|MB_ICONEXCLAMATION "無法把 IPS 複製到安裝資料夾（來源檔可能正在使用中，或磁碟空間不足）：$\r$\n$IpsPath$\r$\n$\r$\n安裝會照常完成；第一次開啟遊戲時，啟動器會說明原因並再請你選擇。" /SD IDOK
      ${EndIf}
    ${EndIf}
  ${EndIf}

  ; 「新增或移除程式」的項目，以及本程式自己記的上次安裝位置；之前測試版寫的 UninstallerId、SaveOrigin 不再使用
  ${GetSize} "$INSTDIR" "/M=${EXE_NAME} /S=0K /G=0" $0 $1 $2
  !insertmacro WRITE_UNINSTALL_ENTRY "${DISPLAY_VERSION}" "$0"
  DeleteRegValue HKCU "${UNINSTALL_KEY}" "UninstallerId"
  DeleteRegValue HKCU "${UNINSTALL_KEY}" "SaveOrigin"
  WriteRegStr HKCU "${OWN_KEY}" "InstallDir" "$INSTDIR"
  Push "安裝完成：$INSTDIR"
  Call Log
SectionEnd

; ======================================================================
; 解除安裝
; ======================================================================
; 解除安裝的順序：un.onInit 確認 $INSTDIR 有 state=installed 的有效標記檔（沒有就什麼都不做，結束碼 21）→
; 「新增或移除程式」項目與捷徑（只在這一份擁有它們時）→ 程式檔與（有勾的話）玩家資料 → 解除安裝程式 → 最後才處理標記檔：
; 資料夾裡除了標記檔之外什麼都不剩 → 刪標記檔、移除空資料夾；否則（含只剩用 _?= 直接執行、刪不掉自己的解除安裝程式）
; 標記檔改成 state=removed。
; 清單內有刪不掉的（正在執行的解除安裝程式本身不算）→ 標記檔停在 state=installed，項目寫回、解除安裝程式保留，結束碼 20。
; 狀態只在 un.onInit 算一次，存進下面的變數
Var U_OwnsEntry      ; "1"：這一份擁有「新增或移除程式」項目、捷徑與 %LOCALAPPDATA%\dbz-ssd-zhtw（項目指向這裡、
                     ;      沒有項目，或項目指向的地方已沒有 state=installed 的有效標記檔）
Var U_EntryRemoved   ; "1"：這次刪了項目（有殘留時寫回）
Var U_EntryVer       ; 刪項目前讀到的 DisplayVersion、EstimatedSize（寫回用）
Var U_EntrySize
Var U_Installed      ; 自己標記檔的 installed=（寫回 state=removed 時沿用）
Var U_Block          ; 「刪玩家資料」停用的原因（空字串＝可用）
Var U_DidData        ; "1"：這次有刪玩家資料（殘留檢查要檢查資料清單）
Var U_DataList       ; 確認訊息與紀錄用的刪除範圍
Var U_Leftover       ; 清單內刪不掉的路徑
Var U_WantData       ; "1"：命令列有單獨的 /DELETEDATA
Var U_SelfExe        ; "1"：正在執行的就是 $INSTDIR\${UNINSTALLER}（用 _?= 直接執行），它刪不掉自己
Var U_Mode           ; 清單函式的動作："delete"（刪）／"check"（還在的記進 $U_Leftover）

; 元件頁的兩項：程式本體（固定）與「同時刪除玩家資料」（Section /o：預設不勾）
Section "un.程式本體（啟動器、捷徑與「新增或移除程式」的登記）" SEC_UN_MAIN
  SectionIn RO
  SetShellVarContext current
  ; 1. 「新增或移除程式」項目與捷徑：只在這一份擁有它們時（兩處各裝一份、或在複製出來的副本上解除時，不動另一份的）
  ${If} $U_OwnsEntry == "1"
    ReadRegStr $U_EntryVer HKCU "${UNINSTALL_KEY}" "DisplayVersion"
    ReadRegDWORD $U_EntrySize HKCU "${UNINSTALL_KEY}" "EstimatedSize"
    ${If} $U_EntryVer == ""
      StrCpy $U_EntryVer "${DISPLAY_VERSION}"
    ${EndIf}
    DeleteRegKey HKCU "${UNINSTALL_KEY}"
    StrCpy $U_EntryRemoved "1"
    Delete "${SM_DIR}\${PRODUCT}.lnk"
    Delete "${SM_DIR}\${HDPACK_LNK}.lnk"
    RMDir "${SM_DIR}"
    ; 先前測試版的開始功能表捷徑位置
    Delete "$SMPROGRAMS\${PRODUCT}.lnk"
    Delete "$DESKTOP\${PRODUCT}.lnk"
  ${Else}
    Push "「新增或移除程式」的項目指向另一個安裝，保留它、捷徑與 %LOCALAPPDATA%\dbz-ssd-zhtw"
    Call un.Log
  ${EndIf}
  ; 2. 程式檔
  StrCpy $U_Mode "delete"
  Call un.ProgramItems
SectionEnd

Section /o "un.同時刪除存檔、ROM 設定、HD 美術包與遊戲設定" SEC_UN_DATA
  SetShellVarContext current
  ${If} $U_Block != ""
    Push "不刪玩家資料：$U_Block"
    Call un.Log
    Return
  ${EndIf}
  Push "刪除玩家資料（只刪本程式寫的檔案與資料夾）：$\r$\n$U_DataList"
  Call un.Log
  StrCpy $U_DidData "1"
  StrCpy $U_Mode "delete"
  Call un.DataItems
SectionEnd

; 最後：殘留檢查 → 解除安裝程式 → 標記檔
Section "-un.Finish"
  SetShellVarContext current
  StrCpy $U_Leftover ""
  StrCpy $U_Mode "check"
  Call un.ProgramItems
  ${If} $U_DidData == "1"
    Call un.DataItems
  ${EndIf}
  ${If} $U_Leftover == ""
  ${AndIf} $U_SelfExe != "1"
    Delete "$INSTDIR\${UNINSTALLER}"
    Push "$INSTDIR\${UNINSTALLER}"
    Call un.NoteLeftover
  ${EndIf}

  ${If} $U_Leftover != ""
    ; 停在 state=installed（標記檔不動），項目寫回、解除安裝程式保留：排除原因後可以再執行一次
    ${If} $U_EntryRemoved == "1"
      !insertmacro WRITE_UNINSTALL_ENTRY "$U_EntryVer" "$U_EntrySize"
      ; 捷徑只在啟動器 exe 還在時補回（不在就指不到東西）
      ${If} ${FileExists} "$INSTDIR\${EXE_NAME}"
        !insertmacro WRITE_SHORTCUTS
      ${EndIf}
    ${EndIf}
    Push "沒有刪乾淨（結束碼 ${EXIT_LEFTOVER}）：$U_Leftover$\r$\n標記檔維持 state=installed，解除安裝程式與「新增或移除程式」的登記保留，排除原因後可以再執行一次解除安裝"
    Call un.Log
    SetErrorLevel ${EXIT_LEFTOVER}
    MessageBox MB_OK|MB_ICONEXCLAMATION "下面這些沒有刪掉：$U_Leftover$\r$\n$\r$\n可能是檔案正在使用中（例如遊戲還開著），或沒有權限。關掉遊戲後再執行一次解除安裝（「新增或移除程式」裡的項目還在）。若某個項目是連結（連接點），只會移除連結本身，它指向的內容不會刪。" /SD IDOK
  ${Else}
    Call un.OnlyMarkerLeft
    Pop $0
    ${If} $0 == "1"
      Delete "$INSTDIR\${MARKER_NAME}"
      RMDir "$INSTDIR"
      ; 完全移除：本程式記的上次安裝位置若是這裡，也清掉
      ReadRegStr $1 HKCU "${OWN_KEY}" "InstallDir"
      ${If} $1 != ""
        Push $1
        Push $INSTDIR
        Call un.SamePath
        Pop $1
        ${If} $1 == "1"
          DeleteRegKey HKCU "${OWN_KEY}"
        ${EndIf}
      ${EndIf}
      ${If} ${FileExists} "$INSTDIR\*.*"
        Push "解除安裝完成：標記檔已刪；$INSTDIR 只剩 desktop.ini／Thumbs.db，資料夾留著"
      ${Else}
        Push "解除安裝完成，已完全移除：$INSTDIR"
      ${EndIf}
      Call un.Log
    ${Else}
      StrCpy $MkInstalled $U_Installed
      Push "removed"
      Call un.WriteMarker
      Pop $0
      ${If} $0 == "ok"
        Push "解除安裝完成：$INSTDIR 還有東西（例如保留的存檔），留下標記檔 state=removed，之後在這裡重裝就認得"
        Call un.Log
      ${Else}
        Push "沒有完成（結束碼 ${EXIT_LEFTOVER}）：無法把標記檔 $INSTDIR\${MARKER_NAME} 改成 state=removed（可能正在使用中或沒有權限）；它仍是 state=installed"
        Call un.Log
        SetErrorLevel ${EXIT_LEFTOVER}
        MessageBox MB_OK|MB_ICONEXCLAMATION "程式已移除，但無法更新安裝資料夾裡的標記檔：$\r$\n$INSTDIR\${MARKER_NAME}$\r$\n$\r$\n它仍標示為「已安裝」。在同一個位置重裝不受影響；要讓它標示為已解除，請關掉正在使用它的程式後在同一個位置重裝，再解除安裝一次。" /SD IDOK
      ${EndIf}
    ${EndIf}
  ${EndIf}
SectionEnd

; Call un.OnlyMarkerLeft；Pop 得到 "1"：$INSTDIR 裡除了標記檔之外沒有別的。用 _?= 直接執行時留下的解除安裝程式本身
; 也是本程式的東西：還在就保留標記檔（state=removed），重裝才認得這個資料夾
Function un.OnlyMarkerLeft
  Push $0
  Push $1
  Push $2
  StrCpy $0 "1"
  ClearErrors
  FindFirst $1 $2 "$INSTDIR\*.*"
  ${DoWhile} $2 != ""
    ${If} $2 != "."
    ${AndIf} $2 != ".."
    ${AndIf} $2 != "${MARKER_NAME}"
    ${AndIf} $2 != "desktop.ini"
    ${AndIf} $2 != "Thumbs.db"
      StrCpy $0 "0"
      ${ExitDo}
    ${EndIf}
    FindNext $1 $2
  ${Loop}
  FindClose $1
  Pop $2
  Pop $1
  Exch $0
FunctionEnd

; ======================================================================
; 本程式（安裝程式加啟動器）會寫出的檔名與子資料夾：解除安裝只刪這份清單裡的名字。
; 子資料夾整個刪（un.RemoveTree，不跟進連接點）；save\ 根層與安裝資料夾裡清單外的東西一律留下，save\ 清完是空的才移除。
; 出處：launcher/src/main.rs、preset.rs、hd_offer.rs、update.rs；上游遊戲 crates/ssd-app/src/paths.rs、packs.rs、options.rs
; ======================================================================

; Push 路徑；Call un.ItemFile／un.ItemDir：依 $U_Mode 刪掉（delete）或還在就記進 $U_Leftover（check）
Function un.ItemFile
  Exch $0
  ${If} $U_Mode == "delete"
    ${If} ${FileExists} "$0"
      SetFileAttributes "$0" NORMAL
      Delete "$0"
    ${EndIf}
  ${Else}
    Push $0
    Call un.NoteLeftover
  ${EndIf}
  Pop $0
FunctionEnd

Function un.ItemDir
  Exch $0
  ${If} $U_Mode == "delete"
    Push $0
    Call un.RemoveTree
  ${Else}
    Push $0
    Call un.NoteLeftover
  ${EndIf}
  Pop $0
FunctionEnd

; Push 資料夾；Push 檔名開頭；Call un.ItemPrefix：那個資料夾裡檔名以它開頭的檔，各當成一個 un.ItemFile
Function un.ItemPrefix
  Exch $1
  Exch
  Exch $0
  Push $2
  Push $3
  ClearErrors
  FindFirst $2 $3 "$0\$1*"
  ${DoWhile} $3 != ""
    Push "$0\$3"
    Call un.ItemFile
    FindNext $2 $3
  ${Loop}
  FindClose $2
  Pop $3
  Pop $2
  Pop $0
  Pop $1
FunctionEnd

; 程式本體：不論有沒有勾「同時刪除」都刪
Function un.ProgramItems
  Push "$INSTDIR\${EXE_NAME}"                ; 安裝程式放的啟動器
  Call un.ItemFile
  Push "$INSTDIR\${EXE_STEM}.previous.exe"   ; 自動更新留下的上一版（update.rs:412）
  Call un.ItemFile
  Push "$INSTDIR\${EXE_STEM}.update.tmp"     ; 自動更新下載中的新版（update.rs:411）
  Call un.ItemFile
  Push "$INSTDIR\hd-pack.zip.part"           ; HD 美術包下載暫存（hd_offer.rs:41 PART_NAME）
  Call un.ItemFile
FunctionEnd

; 玩家資料：只在勾了「同時刪除」時
Function un.DataItems
  Push $0
  Push $1
  StrCpy $0 "$INSTDIR\save"                  ; 啟動器的資料夾（main.rs:84 SAVE_DIR，exe 旁的 save\）
  ; save\ 裡的子資料夾（整個刪）
  Push "$0\saves\ssd.settings.tmp" ; 啟動器改寫設定檔的暫存檔，中途失敗才會留下（main.rs:796 settle_settings）；也隨 saves\ 整個刪
  Call un.ItemFile
  Push "$0\saves"          ; 存檔 ssd.sav、ssd.options、ssd.settings、章節存檔（paths.rs:34、options.rs:20-21、chapter.rs:72）
  Call un.ItemDir
  Push "$0\states"         ; 即時存檔 slotN.ssdstate（paths.rs:38、hotkeys.rs:63）
  Call un.ItemDir
  Push "$0\cache"          ; ROM 複本 cache\rom\canonical.sfc、解開的 HD 美術包、cache\mods（paths.rs:30、hd_install.rs:44、main.rs:179）
  Call un.ItemDir
  Push "$0\mods"           ; 安裝的資料包（packs.rs:35 ssd_mod::set::FOLDER；main.rs:318 從原版遊戲搬來）
  Call un.ItemDir
  Push "$0\art-override"   ; 美術覆蓋圖（main.rs:662 ART_OVERRIDE）
  Call un.ItemDir
  Push "$0\voice"          ; 變身語音（main.rs:909 VOICE）
  Call un.ItemDir
  ; save\ 根層的檔
  Push "$0\${SYNC_MARKER}" ; 同步標記（main.rs:548 SYNC_MARKER）
  Call un.ItemFile
  Push "$0\.launcher-art" ; 美術覆蓋圖的紀錄，放在 art-override\ 旁邊（main.rs:698 ART_MARKER、:757、:810-812）
  Call un.ItemFile
  Push "$0\.launcher-lang" ; 語言設定的標記：之前的啟動器的玩家，語言只在每個資料夾定一次（main.rs:655 LANGUAGE_MARKER）
  Call un.ItemFile
  ; 同步的譯文表與字型，被換掉時舊的改名 .bak（main.rs:480 SYNCED、:544 TABLES）
  Push "$0\zh-tw.tsv"
  Call un.ItemFile
  Push "$0\zh-tw.tsv.bak"
  Call un.ItemFile
  Push "$0\ui-zh-tw.tsv"
  Call un.ItemFile
  Push "$0\ui-zh-tw.tsv.bak"
  Call un.ItemFile
  Push "$0\fonts\LXGWWenKaiTC-Regular.ttf"
  Call un.ItemFile
  Push "$0\fonts\LXGWWenKaiTC-Regular.ttf.bak"
  Call un.ItemFile
  Push "$0\fonts\OFL.txt"
  Call un.ItemFile
  Push "$0\fonts\OFL.txt.bak"
  Call un.ItemFile
  ; 港版譯文表：這一版啟動器同步的，被換掉時舊的改名 .bak（main.rs:486 SYNCED）
  Push "$0\zh-hk.tsv"
  Call un.ItemFile
  Push "$0\zh-hk.tsv.bak"
  Call un.ItemFile
  Push "$0\ui-zh-hk.tsv"
  Call un.ItemFile
  Push "$0\ui-zh-hk.tsv.bak"
  Call un.ItemFile
  ; 安裝程式預放的 ROM／IPS（preset.rs:16-19 SETUP_DIR、ROM_NAMES、IPS_NAME）
  Push "$0\setup\rom.sfc"
  Call un.ItemFile
  Push "$0\setup\rom.smc"
  Call un.ItemFile
  Push "$0\setup\${IPS_NAME}"
  Call un.ItemFile
  ; 啟動器的寫入測試檔 .write-test-<pid>（main.rs:136；正常會立刻刪掉，當掉時才會留下）
  Push "$0"
  Push ".write-test-"
  Call un.ItemPrefix
  ${If} $U_Mode == "delete"
    RMDir "$0\fonts"
    RMDir "$0\setup"
    RMDir "$0"            ; 清完是空的才移除
  ${EndIf}
  ; 安裝資料夾（exe 旁）：HD 美術包與舊包備份 .old、被佔用時 .old2～.old99（hd_offer.rs:39 FILE_NAME、:45 BACKUP_NAME、backup_path）
  Push "$INSTDIR\hd-pack.zip"
  Call un.ItemFile
  Push "$INSTDIR\hd-pack.zip.old"
  Call un.ItemFile
  StrCpy $1 2
  ${Do}
    Push "$INSTDIR\hd-pack.zip.old$1"
    Call un.ItemFile
    IntOp $1 $1 + 1
    ${If} $1 > 99
      ${ExitDo}
    ${EndIf}
  ${Loop}
  ; 啟動器自己的資料夾（解開的遊戲本體、紀錄與設定；main.rs:79-80 launcher_root）：只在這一份擁有共用項目時
  ${If} $U_OwnsEntry == "1"
  ${AndIf} $LOCALAPPDATA != ""
    Push "$LOCALAPPDATA\dbz-ssd-zhtw"
    Call un.ItemDir
  ${EndIf}
  Pop $1
  Pop $0
FunctionEnd

; Push 路徑；Call un.NoteLeftover：還在的話加進 $U_Leftover
Function un.NoteLeftover
  Exch $0
  Push $1
  Push $0
  Call un.PathState
  Pop $1
  ${If} $1 != "none"
    StrCpy $U_Leftover "$U_Leftover$\r$\n$0"
  ${EndIf}
  Pop $1
  Pop $0
FunctionEnd

; $R9＝參數；設定 $U_WantData：只有單獨一個 /DELETEDATA（前後是空白或字串頭尾，不分大小寫）才算。
; 不用 GetOptions：它只比前綴（/DELETEDATA=0、/DeleteDataFalse 也算），而且前面參數裡的撇號會把後面吞掉。
; 有 /DELETEDATA 字樣卻不是單獨參數的，寫進紀錄說明沒有採用
Function un.ScanDeleteData
  Push $0
  Push $1
  Push $2
  Push $3
  Push $4
  Push $5
  StrCpy $U_WantData "0"
  StrCpy $0 " $R9 "
  StrLen $1 $0
  StrCpy $2 1
  ${Do}
    ${If} $2 >= $1
      ${ExitDo}
    ${EndIf}
    StrCpy $3 $0 11 $2
    ${If} $3 == "/DELETEDATA"
      IntOp $4 $2 - 1
      StrCpy $4 $0 1 $4
      IntOp $5 $2 + 11
      StrCpy $5 $0 1 $5
      ${If} $4 == " "
      ${AndIf} $5 == " "
        StrCpy $U_WantData "1"
      ${Else}
        ; 記下這個參數（到下一個空白為止）
        StrCpy $3 ""
        IntOp $4 $2 - 1
        ${Do}
          StrCpy $5 $0 1 $4
          ${If} $5 == " "
          ${OrIf} $4 < 0
            ${ExitDo}
          ${EndIf}
          IntOp $4 $4 - 1
        ${Loop}
        IntOp $4 $4 + 1
        ${Do}
          StrCpy $5 $0 1 $4
          ${If} $5 == " "
          ${OrIf} $5 == ""
            ${ExitDo}
          ${EndIf}
          StrCpy $3 "$3$5"
          IntOp $4 $4 + 1
        ${Loop}
        Push "命令列的「$3」不是單獨的 /DELETEDATA，沒有採用（不刪玩家資料）"
        Call un.Log
      ${EndIf}
    ${EndIf}
    IntOp $2 $2 + 1
  ${Loop}
  Pop $5
  Pop $4
  Pop $3
  Pop $2
  Pop $1
  Pop $0
FunctionEnd

; $INSTDIR 要有 state=installed 的有效標記檔，否則什麼都不做、以結束碼 21 結束（例如 _?= 指到別的資料夾、已經解除過）。
; 「刪玩家資料」的防呆：安裝位置是磁碟根目錄或在 Windows 資料夾裡時整項停用並寫明原因。靜默時只有帶單獨的 /DELETEDATA 才勾選
Function un.onInit
  SetShellVarContext current
  ${GetParameters} $R9
  Push $R9
  Call un.NormParams
  Pop $R9
  Push $R9
  Push "/LOG="
  Call un.GetOpt
  Pop $LogPath
  ${If} $LogPath == ""
    StrCpy $LogPath "$TEMP\${APP_ID}-uninstall.log"
  ${EndIf}
  Call un.ScanDeleteData

  StrCpy $MkValid "0"
  ${If} $INSTDIR != ""
    Push $INSTDIR
    Call un.ReadMarker
  ${EndIf}
  ${If} $MkValid != "1"
  ${OrIf} $MkState != "installed"
    ; $1：開頭那句。標記檔打不開時不能說「不是」，只能說「無法確認是不是」
    Push $1
    StrCpy $1 "不是"
    ${If} $MkValid == "1"
      StrCpy $0 "標記檔的狀態是 $MkState（已經解除安裝過）"
    ${ElseIf} $MkUnreadable == "1"
      StrCpy $0 "標記檔 ${MARKER_NAME} 打不開（可能是遊戲、防毒軟體或其他程式正在使用它）；關掉它們後再解除安裝一次"
      StrCpy $1 "無法確認是不是"
    ${Else}
      StrCpy $0 "沒有本程式的標記檔"
    ${EndIf}
    Push "中止（結束碼 ${EXIT_NOT_OURS}）：$INSTDIR $1本程式目前的安裝（$0），沒有動任何東西"
    Call un.Log
    MessageBox MB_OK|MB_ICONSTOP "這個資料夾$1本程式目前的安裝位置，解除安裝已中止，沒有刪除任何東西：$\r$\n$INSTDIR$\r$\n$\r$\n$0" /SD IDOK
    Pop $1
    SetErrorLevel ${EXIT_NOT_OURS}
    Quit
  ${EndIf}
  StrCpy $U_Installed $MkInstalled

  ; 「新增或移除程式」項目指向哪裡：這裡、沒有項目，或那裡已沒有 state=installed 的有效標記檔 → 這一份擁有它
  StrCpy $U_OwnsEntry "1"
  ReadRegStr $1 HKCU "${UNINSTALL_KEY}" "InstallLocation"
  ${If} $1 != ""
    Call un.RegisteredHere
    Pop $2
    ${If} $2 != "1"
      ; 讀得到那個位置才判斷：所在的磁碟根（或網路共用）讀不到、資料夾屬性讀不到 → 不確定，當成那一份還在
      ${un.GetRoot} "$1" $3
      Push "$3\"
      Call un.PathState
      Pop $4
      Push $1
      Call un.PathState
      Pop $5
      ${If} $4 != "dir"
      ${OrIf} $5 == "other"
        StrCpy $U_OwnsEntry "0"
        Push "「新增或移除程式」的項目指向 $1，現在讀不到；當成那一份還在，不動它的項目、捷徑與 %LOCALAPPDATA%\dbz-ssd-zhtw"
        Call un.Log
      ${ElseIf} $5 == "dir"
        Push $1
        Call un.ReadMarker
        ${If} $MkUnreadable == "1"
          ; 標記檔在卻打不開：和讀不到一樣當成不確定
          StrCpy $U_OwnsEntry "0"
          Push "「新增或移除程式」的項目指向 $1，那裡的標記檔打不開；當成那一份還在，不動它的項目、捷徑與 %LOCALAPPDATA%\dbz-ssd-zhtw"
          Call un.Log
        ${ElseIf} $MkValid == "1"
        ${AndIf} $MkState == "installed"
          StrCpy $U_OwnsEntry "0"
        ${EndIf}
      ${EndIf}
    ${EndIf}
  ${EndIf}

  StrCpy $U_Block ""
  ${un.GetRoot} "$INSTDIR" $1
  StrLen $2 $WINDIR
  StrCpy $3 $INSTDIR $2
  StrCpy $4 $INSTDIR 1 $2
  ${If} $INSTDIR == $1
  ${OrIf} $INSTDIR == "$1\"
    StrCpy $U_Block "安裝位置是磁碟根目錄"
  ${ElseIf} $3 == $WINDIR
  ${AndIf} $4 == ""
    StrCpy $U_Block "安裝位置是 Windows 資料夾"
  ${ElseIf} $3 == $WINDIR
  ${AndIf} $4 == "\"
    StrCpy $U_Block "安裝位置在 Windows 資料夾裡"
  ${EndIf}

  Push $EXEPATH
  Push "$INSTDIR\${UNINSTALLER}"
  Call un.SamePath
  Pop $U_SelfExe

  ; 確認訊息與紀錄用的刪除範圍（實際刪的是 un.DataItems 的清單）
  StrCpy $U_DataList "$INSTDIR\save\ 裡本程式寫的：存檔 saves\、即時存檔 states\、cache\、mods\、art-override\、voice\、譯文表與字型、預放的 ROM／IPS、.launcher-sync.toml$\r$\n$INSTDIR\hd-pack.zip、hd-pack.zip.old（含 old2～old99）"
  ${If} $U_OwnsEntry == "1"
    StrCpy $U_DataList "$U_DataList$\r$\n$LOCALAPPDATA\dbz-ssd-zhtw\（解開的遊戲本體、紀錄與設定）"
  ${EndIf}

  ${If} $U_Block != ""
    SectionSetFlags ${SEC_UN_DATA} ${SF_RO}
    ; 原因寫在元件頁上方的說明裡（un.DataShow）：樹狀清單的項目不會換行，接在後面會比頁面還寬
    SectionSetText ${SEC_UN_DATA} "同時刪除存檔、ROM 設定、HD 美術包與遊戲設定（無法使用）"
    ${If} $U_WantData == "1"
      Push "已帶 /DELETEDATA，但不刪玩家資料：$U_Block"
      Call un.Log
    ${EndIf}
  ${ElseIf} ${Silent}
  ${AndIf} $U_WantData == "1"
    SectionSetFlags ${SEC_UN_DATA} ${SF_SELECTED}
  ${EndIf}
  Push "解除安裝：$INSTDIR（擁有「新增或移除程式」項目 $U_OwnsEntry；正在執行的就是安裝資料夾裡的解除安裝程式 $U_SelfExe）"
  Call un.Log
FunctionEnd

; 元件頁離開時：勾了「同時刪除」就再確認一次，列出實際會刪的路徑；預設按鈕是「否」，按「否」留在本頁
; 元件頁顯示時：刪玩家資料被停用（$U_Block）就把原因接在上方說明後面，再換外觀（un.SkinPage 依新的長度排版）
Function un.DataShow
  ${If} $U_Block != ""
    Push $0
    Push $1
    FindWindow $0 "#32770" "" $HWNDPARENT
    GetDlgItem $0 $0 1006
    System::Call 'user32::GetWindowTextW(p r0, w .r1, i ${NSIS_MAX_STRLEN}) i'
    SendMessage $0 ${WM_SETTEXT} 0 "STR:$1$\r$\n$\r$\n第二項無法使用：$U_Block"
    Pop $1
    Pop $0
  ${EndIf}
  Call un.SkinPage
FunctionEnd

Function un.DataLeave
  ${If} ${SectionIsSelected} ${SEC_UN_DATA}
    ${IfNot} ${Cmd} `MessageBox MB_YESNO|MB_ICONEXCLAMATION|MB_DEFBUTTON2 "確定要永久刪除下面這些嗎？刪除後無法復原。$\r$\n$\r$\n$U_DataList$\r$\n$\r$\n存檔包含啟動器從原版遊戲搬過來的存檔，會一併刪除。$\r$\n不會刪除：遊戲截圖（含 save\screenshots\）、save 資料夾裡不是本程式寫的檔案與資料夾、環境變數 SSD_DATA_DIR 指定的資料夾、$LOCALAPPDATA\dbz-ssd-recompiled\ 資料夾本身，以及你自己另存的其他美術包檔案。" IDYES`
      Abort
    ${EndIf}
  ${EndIf}
FunctionEnd

; Push 資料夾；Call un.RemoveTree：刪掉資料夾與裡面的一切，但不跟進連接點／符號連結（reparse point）：
; 遇到就只移除連結本身。NSIS 文件沒寫 RMDir /r 對 reparse point 的行為，所以逐項判斷屬性
; （FILE_ATTRIBUTE_REPARSE_POINT 0x400、FILE_ATTRIBUTE_DIRECTORY 0x10）
Function un.RemoveTree
  Exch $0
  Push $1
  Push $2
  Push $3
  Push $4
  System::Call 'kernel32::GetFileAttributesW(w r0) i .r3'
  ${If} $3 <> -1
    IntOp $4 $3 & 0x400
    ${If} $4 <> 0
      RMDir "$0"
    ${Else}
      FindFirst $1 $2 "$0\*.*"
      ${DoWhile} $2 != ""
        ${If} $2 != "."
        ${AndIf} $2 != ".."
          System::Call 'kernel32::GetFileAttributesW(w "$0\$2") i .r3'
          ${If} $3 <> -1
            IntOp $4 $3 & 0x400
            IntOp $3 $3 & 0x10
            ${If} $4 <> 0
              ${If} $3 <> 0
                RMDir "$0\$2"
              ${Else}
                Delete "$0\$2"
              ${EndIf}
            ${ElseIf} $3 <> 0
              Push "$0\$2"
              Call un.RemoveTree
            ${Else}
              SetFileAttributes "$0\$2" NORMAL
              Delete "$0\$2"
            ${EndIf}
          ${EndIf}
        ${EndIf}
        FindNext $1 $2
      ${Loop}
      FindClose $1
      RMDir "$0"
    ${EndIf}
  ${EndIf}
  Pop $4
  Pop $3
  Pop $2
  Pop $1
  Pop $0
FunctionEnd
