//! The language of a run's text (`ssd_menus::Language`): the PC file's choice (the Accessibility page's Language
//! row, or the first launch's picker), unless `--lang` names one for the run. Only the Enhanced mode draws a
//! translation; Classic is the original's English whatever the choice.
//!
//! A translation is the player's files, read at run time (nothing of it is built in), named by the language's code:
//! `<code>.tsv`, the game's text (`ssd_catalogue::translation`), and `ui-<code>.tsv`, the HD menus' words
//! (`ssd_menus::ui_text`, optional), in the data folder (`SSD_DATA_DIR`, the app's folder on Android), else next to
//! the executable, and a font in a `fonts` folder there. Every `<code>.tsv` there that is a translation is a language
//! the player is offered (`zh-hk.tsv` is Traditional Chinese for Hong Kong, as it always was), named by the file's
//! `#name` line and drawn with the font its `#font` line names first ([`ssd_catalogue::translation::Header`]).
//! Before the player chooses (and without `--lang`), the language is the first translation of the first launch's
//! Language page, on every platform ([`interface`]). Without a translation, or for a key it lacks, the text stays
//! English; a missing file is said once in the log.

use crate::paths;
use ssd_catalogue::translation::{Header, Translation};
use ssd_hd::translate::{ChineseFace, Overlay};
use ssd_menus::Language;
use ssd_menus::language::Locale;
use ssd_menus::options::Values;
use ssd_menus::ui_text::UiText;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// The files of the translations the Android APK brings, which `android.rs` installs into the data folder: each
/// language's two tables, packed from `translations/<code>/`, and the font they share, from `translations/zh-tw/fonts`
/// (`android/app/build.gradle`). Here, not in `android.rs`, so that the desktop's tests check it.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub(crate) const APK_TRANSLATION_FILES: &[&str] = &[
    "zh-tw.tsv",
    "ui-zh-tw.tsv",
    "zh-hk.tsv",
    "ui-zh-hk.tsv",
    "fonts/LXGWWenKaiTC-Regular.ttf",
    "fonts/OFL.txt",
];

/// The language `--lang` gave, which wins over the PC file for the run.
static OVERRIDE: Mutex<Option<Language>> = Mutex::new(None);
/// The translations, each read on the first use of its language (`None` inside: looked for and not found, or
/// unreadable), and forgotten by [`configure`]: Android runs the game more than once in a process.
static TABLE: Mutex<Vec<(Language, Option<Arc<Translation>>)>> = Mutex::new(Vec::new());
/// The HD menus' words of each language, read and forgotten as [`TABLE`] is.
static UI_TABLE: Mutex<Vec<(Language, Option<Arc<UiText>>)>> = Mutex::new(Vec::new());
/// The font of the HD menus (the language the run starts in, see [`cjk_font`]), read on first use and kept (the
/// windows' fonts are made from it).
static FONT: Mutex<Lookup<Arc<[u8]>>> = Mutex::new(Lookup::Unread);
/// The fonts made for the translations' text, by font file, on first use.
static FACES: Mutex<Vec<(PathBuf, Option<Arc<ChineseFace>>)>> = Mutex::new(Vec::new());

/// A file looked for once: not yet, or found (`Some`) or not.
enum Lookup<T> {
    Unread,
    Read(Option<T>),
}

/// The value of `slot`, read with `read` on first use.
fn looked_up<T: Clone>(slot: &Mutex<Lookup<T>>, read: impl FnOnce() -> Option<T>) -> Option<T> {
    let mut slot = slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Lookup::Unread = *slot {
        *slot = Lookup::Read(read());
    }
    match &*slot {
        Lookup::Read(value) => value.clone(),
        Lookup::Unread => None,
    }
}

/// The value of `key` in `slot`, read with `read` on the first use of `key`.
fn looked_up_for<K: PartialEq, T: Clone>(
    slot: &Mutex<Vec<(K, Option<T>)>>,
    key: K,
    read: impl FnOnce() -> Option<T>,
) -> Option<T> {
    let mut slot = slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((_, value)) = slot.iter().find(|(read_for, _)| *read_for == key) {
        return value.clone();
    }
    let value = read();
    slot.push((key, value.clone()));
    value
}

/// Sets the language of the command line (`None`: the PC file's), forgets the files read by an earlier run, and
/// offers in the Options' Language row (and the first launch's choice) the translations the player has
/// ([`offered`]).
pub fn configure(language: Option<Language>) {
    ssd_menus::options::offer_languages(offered());
    *OVERRIDE.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = language;
    TABLE.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clear();
    UI_TABLE.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clear();
}
/// The language the command line gave, if it gave one.
pub fn overridden() -> Option<Language> {
    *OVERRIDE.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The language of a run whose PC file holds `values` (the game's text and the HD menus): the command line's, else
/// the file's; before any is chosen, the first translation of the first launch's Language page, so that the page and
/// its font are those of a translation the player has; English without one ([`interface_in`]).
pub fn effective(values: &Values) -> Language {
    effective_in(values, &offered())
}

/// [`effective`] with the translations offered given.
fn effective_in(values: &Values, offered: &[Locale]) -> Language {
    interface_in(overridden(), Language::from_values(values), offered)
}

/// The language a run takes at once when its PC file comes to hold `values` (the Options changed a row): the stored
/// one, which wins over `--lang` from then on; with none (the Language row reset) the one a run starts in
/// ([`effective`]: `--lang`, else the first translation, English without one), as the next start does before it asks.
/// Nothing is written: the file keeps no language, and the next start asks.
pub fn followed(values: &Values) -> Language {
    followed_in(values, &offered())
}

/// [`followed`] with the translations offered given.
fn followed_in(values: &Values, offered: &[Locale]) -> Language {
    Language::from_values(values).unwrap_or_else(|| effective_in(values, offered))
}

/// The language of the app's own words around the game (the touch controls, the first launch's screens) where the PC
/// file holds `values`: the command line's, else the file's; before any is chosen, the first translation of the first
/// launch's Language page, English without one ([`interface_in`]); the same as [`effective`].
pub fn interface(values: &Values) -> Language {
    interface_in(overridden(), Language::from_values(values), &offered())
}

/// [`interface`] from its parts: the command line's language, the PC file's, and the translations offered. Before a
/// choice, the first translation of the first launch's Language page ([`ssd_menus::setup::language_before_a_choice`]:
/// Traditional Chinese (Taiwan) where it is offered, else the first by code), on every platform; English where the
/// player has no translation (as upstream). The Options' Language row shows the same while nothing is stored.
fn interface_in(
    command_line: Option<Language>,
    saved: Option<Language>,
    offered: &[Locale],
) -> Language {
    command_line.or(saved).unwrap_or_else(|| ssd_menus::setup::language_before_a_choice(offered))
}

/// The words of the app's own screens in [`interface`]'s language: the HD menus' table ([`ui_text`]); `None` for
/// English.
pub fn interface_text(values: &Values) -> Option<Arc<UiText>> {
    ui_text(interface(values))
}

/// The folders the player's files of the translation are looked for in, in order: the data folder, then the
/// executable's.
fn places() -> Vec<PathBuf> {
    let mut places = vec![paths::data_dir()];
    if let Some(dir) = std::env::current_exe().ok().and_then(|exe| exe.parent().map(PathBuf::from))
    {
        places.push(dir);
    }
    places
}

/// The first of `places` that holds the file `name`.
fn find_in(places: &[PathBuf], name: &str) -> Option<PathBuf> {
    places.iter().map(|dir| dir.join(name)).find(|path| path.is_file())
}

/// The file of the game's text in `language`: `<code>.tsv`; `None` for English.
#[must_use]
pub fn table_file(language: Language) -> Option<String> {
    (!language.is_english()).then(|| format!("{}.tsv", language.code()))
}

/// The file of the HD menus' words in `language`: `ui-<code>.tsv`; `None` for English.
#[must_use]
pub fn ui_file(language: Language) -> Option<String> {
    (!language.is_english()).then(|| format!("ui-{}.tsv", language.code()))
}

/// A translation found in a folder: its language with the name it shows, and the font its file names first.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Found {
    locale: Locale,
    font: Option<String>,
}

/// The translations in `places`, sorted by code: each `<code>.tsv` whose name is a language's code (lower case, as
/// `--lang` writes it) and whose text is a translation (its columns after its header), the first place's file for a
/// code. `en.tsv` (English is built in) and the menus' `ui-*.tsv` are not languages. A translation under another name
/// is not one either, and the log says which name it should have ([`suggested_name`]).
fn found_in(places: &[PathBuf]) -> Vec<Found> {
    let mut found: Vec<Found> = Vec::new();
    for dir in places {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        let mut paths: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
        paths.sort();
        for path in paths {
            let Some(file) = path.file_name().and_then(|name| name.to_str()) else { continue };
            let Some(code) = file.strip_suffix(".tsv") else { continue };
            let Some(language) =
                Language::from_code(code).filter(|language| language.code() == code)
            else {
                if let Some(name) = suggested_name(file)
                    && let Ok(Some(_)) = Header::read(&path)
                {
                    log::info!("{file} is not a language file name: rename it {name}");
                }
                continue;
            };
            if language.is_english() || found.iter().any(|known| known.locale.language == language)
            {
                continue;
            }
            match Header::read(&path) {
                Ok(Some(header)) => found.push(Found {
                    locale: Locale {
                        language,
                        name: header.name.unwrap_or_else(|| language.default_name()),
                    },
                    font: header.font,
                }),
                Ok(None) => {
                    log::info!("{}: not a translation (no key and zh columns)", path.display());
                }
                Err(error) => log::warn!("{}: {error}", path.display()),
            }
        }
    }
    found.sort_by_key(|known| known.locale.language);
    found
}

/// The name a `.tsv` should have when its own name is not a language's code as `--lang` writes it (lower case letters,
/// digits and `-`): `zh_TW.tsv` should be `zh-tw.tsv`. `None` for a name that is one already, for English (built in),
/// for the menus' `ui-*` files, and for a name no code can be made of.
fn suggested_name(file: &str) -> Option<String> {
    let stem = file.strip_suffix(".tsv")?;
    let language = Language::from_code(&code_of(stem))?;
    (!language.is_english() && language.code() != stem).then(|| format!("{}.tsv", language.code()))
}

/// `stem` made into a code: lower case, each run of other characters (`_`, `.`, a space) one `-`, no `-` at either end.
fn code_of(stem: &str) -> String {
    let mut code = String::new();
    for c in stem.chars().map(|c| c.to_ascii_lowercase()) {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            code.push(c);
        } else if !code.ends_with('-') {
            code.push('-');
        }
    }
    code.trim_matches('-').to_owned()
}

/// The translations of `places` the player can be offered: those with a font to draw them ([`find_font`]).
fn offered_in(places: &[PathBuf]) -> Vec<Locale> {
    found_in(places)
        .into_iter()
        .filter(|known| find_font(places, known.font.as_deref()).is_some())
        .map(|known| known.locale)
        .collect()
}

/// The translations the player has, sorted by code (see [`offered_in`]). With none the language is never asked and
/// the Options have no Language row, so the game is as it is without this feature.
pub fn offered() -> Vec<Locale> {
    offered_in(&places())
}

/// The translation of `language`, read on first use; `None` for English, and when the file is missing or cannot be
/// read (said once in the log: the text stays English).
pub fn translation(language: Language) -> Option<Arc<Translation>> {
    let file = table_file(language)?;
    looked_up_for(&TABLE, language, || {
        let places = places();
        let Some(path) = find_in(&places, &file) else {
            let looked: Vec<String> = places.iter().map(|dir| dir.display().to_string()).collect();
            log::warn!("no {file} in {}: the text stays English", looked.join(" or "));
            return None;
        };
        match Translation::read(&path) {
            Ok(table) => {
                log::info!("translation: {} texts from {}", table.len(), path.display());
                Some(Arc::new(table))
            }
            Err(error) => {
                log::warn!("{}: {error}; the text stays English", path.display());
                None
            }
        }
    })
}

/// The HD menus' words in `language`, read on first use; `None` for English, and when the file is missing or cannot
/// be read (said once in the log: the menus stay English).
pub fn ui_text(language: Language) -> Option<Arc<UiText>> {
    let file = ui_file(language)?;
    looked_up_for(&UI_TABLE, language, || read_ui_text(&places(), &file))
}

/// The menus' table of the first of `places` that holds the file `file` ([`ui_file`]).
fn read_ui_text(places: &[PathBuf], file: &str) -> Option<Arc<UiText>> {
    let Some(path) = find_in(places, file) else {
        log::warn!("no {file} beside the translation: the HD menus stay English");
        return None;
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            let table = UiText::parse(&text);
            log::info!("menus: {} rows from {}", table.len(), path.display());
            Some(Arc::new(table))
        }
        Err(error) => {
            log::warn!("{}: {error}; the HD menus stay English", path.display());
            None
        }
    }
}

/// The rank of a font file of the `fonts` folder of a place (the lowest is taken): a file whose name starts with
/// `first` (a translation's `#font`, any case), then Noto Sans HK (SIL OFL), then Noto Sans TC, the regular weight
/// first in each. Fonts of the player's, never built in.
fn font_rank(name: &str, first: Option<&str>) -> Option<u8> {
    let extension = std::path::Path::new(name).extension()?;
    if !(extension.eq_ignore_ascii_case("ttf") || extension.eq_ignore_ascii_case("otf")) {
        return None;
    }
    let lower = name.to_ascii_lowercase();
    let first = first.map(str::to_ascii_lowercase).filter(|first| !first.is_empty());
    let family = if first.is_some_and(|first| lower.starts_with(&first)) {
        0
    } else if lower.starts_with("notosanshk") {
        2
    } else if lower.starts_with("notosanstc") {
        4
    } else {
        return None;
    };
    Some(family + u8::from(!lower.contains("-regular")))
}

/// The best font file of the `fonts` folders of `places` for a translation whose file names `first` (see
/// [`font_rank`]).
fn find_font(places: &[PathBuf], first: Option<&str>) -> Option<PathBuf> {
    places
        .iter()
        .filter_map(|dir| std::fs::read_dir(dir.join("fonts")).ok())
        .flat_map(|entries| entries.flatten().map(|entry| entry.path()))
        .filter_map(|path| {
            let rank = font_rank(path.file_name()?.to_str()?, first)?;
            Some((rank, path))
        })
        .min_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)))
        .map(|(_, path)| path)
}

/// The font file of `language` in `places`: the one its translation's `#font` names first, else as the search has
/// always been (see [`font_rank`]).
fn font_of(places: &[PathBuf], language: Language) -> Option<PathBuf> {
    let first = table_file(language)
        .and_then(|file| find_in(places, &file))
        .and_then(|path| Header::read(&path).ok().flatten())
        .and_then(|header| header.font);
    find_font(places, first.as_deref())
}

/// The bytes of the font file `path`; `None` when it cannot be read (said in the log).
fn read_font(path: &Path) -> Option<Arc<[u8]>> {
    match std::fs::read(path) {
        Ok(bytes) => {
            log::info!("translation font: {}", path.display());
            Some(Arc::from(bytes))
        }
        Err(error) => {
            log::warn!("{}: {error}", path.display());
            None
        }
    }
}

/// The bytes of the font of the language the run starts in (the command line's, else the PC file's, before any is
/// chosen the app's own screens' ([`interface`]); see [`font_of`]), read on first use; `None` when there is none (said
/// once in the log). The HD menus take it as a fallback of their font for the run, and the languages' names are drawn
/// with it.
pub fn cjk_font() -> Option<Arc<[u8]>> {
    looked_up(&FONT, || {
        let language = interface(&crate::options::read_settings().unwrap_or_default());
        let Some(path) = font_of(&places(), language) else {
            log::info!(
                "no fonts/NotoSansHK-*.otf (or the font a translation names) in the data folder: no translation \
                 can be drawn"
            );
            return None;
        };
        read_font(&path)
    })
}

/// The overlay that draws the translation of `language` over the game's message windows and the names in its tile
/// maps (`ssd_hd::translate`), read from the canonical ROM `rom`, with the language's font ([`font_of`]): `None` for
/// English, and without the translation or a font (the text stays English).
pub fn overlay(language: Language, rom: &[u8]) -> Option<Arc<Overlay>> {
    let table = translation(language)?;
    let Some(path) = font_of(&places(), language) else {
        log::warn!("no font for {}: the text stays English", language.code());
        return None;
    };
    let face = looked_up_for(&FACES, path.clone(), || {
        let bytes = read_font(&path)?;
        match ChineseFace::new(bytes.to_vec()) {
            Ok(face) => Some(Arc::new(face)),
            Err(error) => {
                log::warn!(
                    "the font {} cannot be read ({error}): the text stays English",
                    path.display()
                );
                None
            }
        }
    })?;
    let names = ssd_catalogue::engine::screen_names(rom, &table);
    Some(Arc::new(Overlay::new(table, face).with_names(names)))
}

/// The tests that set the process-wide override (or depend on it) hold this lock.
#[cfg(test)]
pub(crate) static LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    /// A made-up translation's text: a header, then the columns and one made-up row.
    fn made_up(header: &str) -> String {
        format!("{header}key\tzh\nnothing:1\t甲\n")
    }

    #[test]
    fn the_command_line_wins_over_the_file_and_english_is_the_default() {
        let _guard = LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        configure(None);
        let mut values = Values::new();
        // no translation offered (given, not the data folder's): English
        assert_eq!(effective_in(&values, &[]), Language::ENGLISH);
        Language::ZH_HK.store(&mut values);
        assert_eq!(effective(&values), Language::ZH_HK);
        configure(Some(Language::ENGLISH));
        assert_eq!(effective(&values), Language::ENGLISH);
        configure(None);
    }

    #[test]
    fn the_game_starts_in_the_first_translation_until_one_is_chosen_on_every_platform() {
        let _guard = LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        configure(None);
        let zh = Language::from_code("zh-tw").unwrap();
        let offered = [Locale::unnamed(zh)];
        let mut values = Values::new();
        assert_eq!(effective_in(&values, &offered), zh, "nothing chosen yet");
        assert_eq!(effective_in(&values, &[]), Language::ENGLISH, "no translation: as upstream");
        Language::ENGLISH.store(&mut values);
        assert_eq!(
            effective_in(&values, &offered),
            Language::ENGLISH,
            "a saved choice wins (the Language row changes it later)"
        );
        configure(Some(Language::ZH_HK));
        assert_eq!(effective_in(&Values::new(), &offered), Language::ZH_HK, "--lang first");
        configure(None);
    }

    #[test]
    fn each_language_has_files_named_by_its_code_and_english_has_none() {
        assert_eq!(table_file(Language::ENGLISH), None);
        assert_eq!(ui_file(Language::ENGLISH), None);
        assert_eq!(table_file(Language::ZH_HK).as_deref(), Some("zh-hk.tsv"), "as it always was");
        assert_eq!(ui_file(Language::ZH_HK).as_deref(), Some("ui-zh-hk.tsv"));
        let made_up = Language::from_code("xx").unwrap();
        assert_eq!(table_file(made_up).as_deref(), Some("xx.tsv"));
        assert_eq!(ui_file(made_up).as_deref(), Some("ui-xx.tsv"));
    }

    #[test]
    fn the_translation_is_read_from_the_first_folder_that_has_it_and_english_reads_none() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let places = [first.path().to_path_buf(), second.path().to_path_buf()];
        let file = table_file(Language::ZH_HK).unwrap();
        assert_eq!(find_in(&places, &file), None);
        std::fs::write(second.path().join(&file), "key\tzh\na\t甲\n").unwrap();
        assert_eq!(find_in(&places, &file), Some(second.path().join(&file)));
        std::fs::write(first.path().join(&file), "key\tzh\n").unwrap();
        assert_eq!(find_in(&places, &file), Some(first.path().join(&file)));
        assert_eq!(translation(Language::ENGLISH), None);
    }

    #[test]
    fn every_translation_in_the_folders_is_found_named_by_its_header_or_its_code_sorted_by_code() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let places = [first.path().to_path_buf(), second.path().to_path_buf()];
        let write = |dir: &tempfile::TempDir, name: &str, text: &str| {
            std::fs::write(dir.path().join(name), text).unwrap();
        };
        write(&first, "zz.tsv", &made_up(""));
        write(&first, "zh-hk.tsv", &made_up(""));
        write(&first, "xx.tsv", &made_up("#name\t測試語\n#font\tMadeUpSans\n"));
        write(&second, "xx.tsv", &made_up("#name\tSecond\n"));
        write(&second, "yy.tsv", &made_up("#name\tWhy\n"));
        write(&first, "ui-xx.tsv", "Talk\t說\n");
        write(&first, "en.tsv", &made_up(""));
        write(&first, "notes.tsv", "what\twhy\n");
        write(&first, "Upper.tsv", &made_up(""));
        write(&first, "xx.txt", &made_up(""));
        let found = found_in(&places);
        let codes: Vec<&str> = found.iter().map(|known| known.locale.language.code()).collect();
        assert_eq!(
            codes,
            ["xx", "yy", "zh-hk", "zz"],
            "sorted by code, the files that are languages"
        );
        let names: Vec<&str> = found.iter().map(|known| known.locale.name.as_str()).collect();
        assert_eq!(
            names,
            ["測試語", "Why", "繁體中文（香港）", "zz"],
            "the header's, else the code"
        );
        assert_eq!(found[0].font.as_deref(), Some("MadeUpSans"), "the first place's file");
        assert_eq!(found[1].font, None);
    }

    #[test]
    fn a_translation_is_offered_only_with_a_font_to_draw_it() {
        let dir = tempfile::tempdir().unwrap();
        let places = [dir.path().to_path_buf()];
        std::fs::write(dir.path().join("xx.tsv"), made_up("#font\tMadeUpSans\n")).unwrap();
        assert!(offered_in(&places).is_empty(), "no font: nothing can be drawn");
        let fonts = dir.path().join("fonts");
        std::fs::create_dir(&fonts).unwrap();
        std::fs::write(fonts.join("MadeUpSans-Regular.ttf"), b"x").unwrap();
        let offered = offered_in(&places);
        assert_eq!(offered, [Locale::unnamed(Language::from_code("xx").unwrap())]);
        std::fs::write(dir.path().join("zh-hk.tsv"), made_up("")).unwrap();
        assert_eq!(offered_in(&places).len(), 1, "Hong Kong has no font of its own here");
        std::fs::write(fonts.join("NotoSansTC-Regular.otf"), b"x").unwrap();
        let codes: Vec<&str> =
            offered_in(&places).iter().map(|locale| locale.language.code()).collect();
        assert_eq!(codes, ["xx", "zh-hk"]);
    }

    #[test]
    fn the_app_screens_language_is_the_chosen_one_else_the_first_translation() {
        let zh = Language::from_code("zh-tw").unwrap();
        let xx = Language::from_code("xx").unwrap();
        let offered = [Locale::unnamed(zh)];
        let english = Some(Language::ENGLISH);
        assert_eq!(interface_in(Some(xx), Some(zh), &offered), xx, "the command line first");
        assert_eq!(interface_in(None, english, &offered), Language::ENGLISH, "a saved choice wins");
        assert_eq!(interface_in(None, None, &offered), zh, "nothing chosen yet");
        assert_eq!(interface_in(None, None, &[]), Language::ENGLISH, "no translation");
    }

    #[test]
    fn before_a_choice_android_words_are_taiwan_when_both_are_there() {
        let _guard = LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let taiwan = Language::from_code("zh-tw").unwrap();
        let offered = [Locale::unnamed(Language::ZH_HK), Locale::unnamed(taiwan)];
        assert_eq!(interface_in(None, None, &offered), taiwan, "the page's first button");
        assert_eq!(effective_in(&Values::new(), &offered), taiwan, "the game's text as well");
        assert_eq!(
            interface_in(None, Some(Language::ZH_HK), &offered),
            Language::ZH_HK,
            "a chosen Hong Kong"
        );
    }

    #[test]
    fn before_a_choice_android_takes_the_only_translation() {
        let offered = [Locale::unnamed(Language::ZH_HK)];
        assert_eq!(interface_in(None, None, &offered), Language::ZH_HK);
    }

    #[test]
    fn before_a_choice_the_desktop_too_takes_the_first_of_the_page_and_english_without_one() {
        let _guard = LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let taiwan = Language::from_code("zh-tw").unwrap();
        let xx = Language::from_code("xx").unwrap();
        let offered = [Locale::unnamed(Language::ZH_HK), Locale::unnamed(taiwan)];
        assert_eq!(interface_in(None, None, &offered), taiwan, "Taiwan first");
        assert_eq!(
            interface_in(None, None, &[Locale::unnamed(Language::ZH_HK), Locale::unnamed(xx)]),
            xx,
            "without Taiwan: the first by code"
        );
        assert_eq!(interface_in(None, None, &[]), Language::ENGLISH, "no translation: as upstream");
        assert_eq!(interface_in(Some(Language::ZH_HK), None, &offered), Language::ZH_HK, "--lang");
        assert_eq!(
            interface_in(None, Some(Language::ENGLISH), &offered),
            Language::ENGLISH,
            "saved"
        );
        assert_eq!(
            effective_in(&Values::new(), &[]),
            Language::ENGLISH,
            "the game's text, no table"
        );
    }

    #[test]
    fn after_a_reset_the_language_row_shows_the_language_the_game_takes_before_a_choice() {
        use ssd_menus::cheats::Cheats;
        use ssd_menus::options::Category;
        let _guard = LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        configure(None);
        let taiwan = Language::from_code("zh-tw").unwrap();
        let shown_after_a_reset = |offered: &[Locale]| {
            ssd_menus::options::offer_languages(offered.to_vec());
            let row = Category::Accessibility
                .rows()
                .iter()
                .find(|row| row.key == Language::KEY)
                .expect("the Language row");
            let (mut values, mut cheats) = (Values::new(), Cheats::neutral());
            Language::ZH_HK.store(&mut values);
            row.reset(&mut values, &mut cheats);
            assert_eq!(Language::from_values(&values), None, "no access.language left");
            let index = row.index(&values, &cheats, 0);
            let shown = ssd_menus::options::offered_languages()[index].language;
            (shown, effective_in(&values, offered))
        };
        let (shown, in_use) =
            shown_after_a_reset(&[Locale::unnamed(Language::ZH_HK), Locale::unnamed(taiwan)]);
        assert_eq!(shown, in_use, "the row shows the language in use");
        assert_eq!(shown, taiwan);
        let (shown, in_use) = shown_after_a_reset(&[Locale::unnamed(Language::ZH_HK)]);
        assert_eq!((shown, in_use), (Language::ZH_HK, Language::ZH_HK), "Hong Kong alone");
        ssd_menus::options::offer_languages(Vec::new());
        configure(None);
    }

    /// The PC file's `values` once the Options' Language row, with `offered` given to the Options, is reset from
    /// `stored`.
    fn reset_from(stored: Language, offered: &[Locale]) -> Values {
        use ssd_menus::cheats::Cheats;
        use ssd_menus::options::Category;
        ssd_menus::options::offer_languages(offered.to_vec());
        let mut values = Values::new();
        stored.store(&mut values);
        if let Some(row) =
            Category::Accessibility.rows().iter().find(|row| row.key == Language::KEY)
        {
            row.reset(&mut values, &mut Cheats::neutral());
        } else {
            values.remove(Language::KEY);
        }
        ssd_menus::options::offer_languages(Vec::new());
        values
    }

    #[test]
    fn a_reset_language_row_follows_the_language_a_run_starts_in() {
        let _guard = LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        configure(None);
        let taiwan = Language::from_code("zh-tw").unwrap();
        let both = [Locale::unnamed(Language::ZH_HK), Locale::unnamed(taiwan)];
        let values = reset_from(Language::ZH_HK, &both);
        assert_eq!(Language::from_values(&values), None, "nothing stored: the next start asks");
        assert_eq!(followed_in(&values, &both), taiwan, "Taiwan at once, as at the next start");
        assert_eq!(followed_in(&values, &both), effective_in(&values, &both));
        let values = reset_from(Language::ZH_HK, &[]);
        assert_eq!(followed_in(&values, &[]), Language::ENGLISH, "no translation: English");
        let mut kept = Values::new();
        Language::ZH_HK.store(&mut kept);
        assert_eq!(followed_in(&kept, &both), Language::ZH_HK, "a stored choice: unchanged");
        let mut gone = Values::new();
        Language::from_code("xx").unwrap().store(&mut gone);
        assert_eq!(
            followed_in(&gone, &both).code(),
            "xx",
            "a stored language whose files are gone: as before (its text stays English)"
        );
        configure(Some(Language::ZH_HK));
        let values = reset_from(taiwan, &both);
        assert_eq!(followed_in(&values, &both), Language::ZH_HK, "--lang: as effective");
        assert_eq!(followed_in(&values, &both), effective_in(&values, &both));
        let mut chosen = Values::new();
        taiwan.store(&mut chosen);
        assert_eq!(followed_in(&chosen, &both), taiwan, "a choice in the Options wins over --lang");
        configure(None);
    }

    #[test]
    fn before_a_choice_the_page_has_the_font_the_first_translation_names() {
        // The desktop's folder as the launcher fills it: both tables naming LXGWWenKaiTC, that font alone.
        let dir = tempfile::tempdir().unwrap();
        let places = [dir.path().to_path_buf()];
        let header = |name: &str| format!("#name\t{name}\n#font\tLXGWWenKaiTC\n");
        std::fs::write(dir.path().join("zh-tw.tsv"), made_up(&header("繁體中文（台灣）"))).unwrap();
        std::fs::write(dir.path().join("zh-hk.tsv"), made_up(&header("繁體中文（香港）"))).unwrap();
        let fonts = dir.path().join("fonts");
        std::fs::create_dir(&fonts).unwrap();
        std::fs::write(fonts.join("LXGWWenKaiTC-Regular.ttf"), b"x").unwrap();
        // The steps of `cjk_font`: the language before a choice, then its font.
        let language = interface_in(None, None, &offered_in(&places));
        assert_eq!(language.code(), "zh-tw");
        assert_eq!(font_of(&places, language), Some(fonts.join("LXGWWenKaiTC-Regular.ttf")));
        assert_eq!(
            font_of(&places, Language::ENGLISH),
            None,
            "English names no font: none is found"
        );
    }

    #[test]
    fn each_translation_brings_both_of_its_tables() {
        // The tables: the files beside the fonts.
        let tables: Vec<&str> = APK_TRANSLATION_FILES
            .iter()
            .copied()
            .filter(|name| !name.starts_with("fonts/"))
            .collect();
        for name in &tables {
            let other = match name.strip_prefix("ui-") {
                Some(game) => game.to_owned(),
                None => format!("ui-{name}"),
            };
            assert!(name.strip_suffix(".tsv").is_some(), "{name} is a table");
            assert!(tables.contains(&other.as_str()), "{name} without {other}");
        }
        for name in ["zh-tw.tsv", "ui-zh-tw.tsv", "zh-hk.tsv", "ui-zh-hk.tsv"] {
            assert!(tables.contains(&name), "{name} is brought");
        }
        // Each file is in a folder `android/app/build.gradle` packs as the APK's assets.
        let translations = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../translations");
        for name in APK_TRANSLATION_FILES {
            let found =
                ["zh-tw", "zh-hk"].iter().any(|dir| translations.join(dir).join(name).is_file());
            assert!(found, "{name} is in translations/zh-tw or translations/zh-hk");
        }
    }

    #[test]
    fn the_menus_table_is_the_players_file_of_the_language_and_english_reads_none() {
        let dir = tempfile::tempdir().unwrap();
        let places = [dir.path().to_path_buf()];
        assert!(read_ui_text(&places, "ui-xx.tsv").is_none(), "no file: the menus stay English");
        std::fs::write(
            dir.path().join("ui-xx.tsv"),
            "# made up\nTalk\t對話\nPage {}/{}\t第{}/{}頁\n",
        )
        .unwrap();
        assert!(read_ui_text(&places, "ui-zh-hk.tsv").is_none(), "the file of another language");
        let table = read_ui_text(&places, "ui-xx.tsv").unwrap();
        assert_eq!(table.len(), 2);
        assert_eq!(table.get("Page 1/2").as_deref(), Some("第1/2頁"));
        assert!(ui_text(Language::ENGLISH).is_none());
    }

    #[test]
    fn the_font_a_translation_names_comes_first_then_noto_sans_hk_then_tc() {
        assert_eq!(font_rank("NotoSansHK-Regular.otf", None), Some(2));
        assert_eq!(font_rank("NotoSansHK-Bold.ttf", None), Some(3));
        assert_eq!(font_rank("NotoSansTC-Regular.ttf", None), Some(4));
        assert_eq!(font_rank("NotoSansHK-Regular.woff2", None), None);
        assert_eq!(font_rank("OFL.txt", None), None);
        assert_eq!(font_rank("MadeUpSans-Regular.otf", None), None, "named by no translation");
        assert_eq!(font_rank("MadeUpSans-Regular.otf", Some("madeupsans")), Some(0), "any case");
        assert_eq!(font_rank("NotoSansTC-Medium.otf", Some("NotoSansTC")), Some(1));
        assert_eq!(
            font_rank("NotoSansTC-Regular.otf", Some("")),
            Some(4),
            "an empty name: as before"
        );
        let dir = tempfile::tempdir().unwrap();
        let fonts = dir.path().join("fonts");
        std::fs::create_dir(&fonts).unwrap();
        let places = [dir.path().to_path_buf()];
        assert_eq!(find_font(&places, None), None);
        for name in ["NotoSansTC-Regular.otf", "NotoSansHK-Medium.otf", "OFL.txt"] {
            std::fs::write(fonts.join(name), b"x").unwrap();
        }
        assert_eq!(find_font(&places, None), Some(fonts.join("NotoSansHK-Medium.otf")));
        assert_eq!(
            find_font(&places, Some("NotoSansTC")),
            Some(fonts.join("NotoSansTC-Regular.otf"))
        );
        assert_eq!(
            find_font(&places, Some("MadeUpSans")),
            Some(fonts.join("NotoSansHK-Medium.otf")),
            "the font named is missing: the search as before"
        );
        std::fs::write(dir.path().join("xx.tsv"), made_up("#font\tNotoSansTC\n")).unwrap();
        std::fs::write(dir.path().join("zh-hk.tsv"), made_up("")).unwrap();
        let xx = Language::from_code("xx").unwrap();
        assert_eq!(font_of(&places, xx), Some(fonts.join("NotoSansTC-Regular.otf")));
        assert_eq!(font_of(&places, Language::ZH_HK), Some(fonts.join("NotoSansHK-Medium.otf")));
    }

    #[test]
    fn a_translation_under_a_name_that_is_no_code_is_named_with_the_code_it_should_have() {
        assert_eq!(suggested_name("zh_TW.tsv").as_deref(), Some("zh-tw.tsv"));
        assert_eq!(suggested_name("ZH-HK.tsv").as_deref(), Some("zh-hk.tsv"));
        assert_eq!(suggested_name("xx_yy.tsv").as_deref(), Some("xx-yy.tsv"));
        assert_eq!(suggested_name("zh.hk.tsv").as_deref(), Some("zh-hk.tsv"));
        assert_eq!(suggested_name("zh-tw.tsv"), None, "a code already");
        assert_eq!(suggested_name("en.tsv"), None, "English is built in");
        assert_eq!(suggested_name("ui-zh-tw.tsv"), None, "the menus' words");
        assert_eq!(suggested_name("ui_zh_tw.tsv"), None, "the menus' words, misnamed");
        assert_eq!(suggested_name("123.tsv"), None, "no letter, so no code");
        assert_eq!(suggested_name("zh_TW.txt"), None, "not a table");
    }

    #[test]
    fn a_misnamed_translation_is_not_offered_beside_the_file_named_as_its_code() {
        let dir = tempfile::tempdir().unwrap();
        let places = [dir.path().to_path_buf()];
        std::fs::write(dir.path().join("zh_TW.tsv"), made_up("#name\tMisnamed\n")).unwrap();
        std::fs::write(dir.path().join("zh-tw.tsv"), made_up("#name\tRight\n")).unwrap();
        let found = found_in(&places);
        let codes: Vec<&str> = found.iter().map(|known| known.locale.language.code()).collect();
        assert_eq!(codes, ["zh-tw"], "the file named as a code is the language");
        assert_eq!(found[0].locale.name, "Right");
    }
}
