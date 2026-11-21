//! Which graphics backend an Android start uses, from what the last start left behind (`android_report.rs` keeps the
//! files): while a start has not drawn its first picture, a stage file says how far it got; the first picture deletes
//! it. A stage file found at the next start means that start died before its first picture (a crash in a driver as
//! much as a panic), and the game then starts on OpenGL ES instead of Vulkan, remembers that for this installation of
//! the app, and says so. If OpenGL ES dies too, the next start says that neither backend could start the game, and
//! stays on OpenGL ES: it never goes back and forth between the two.
//!
//! The choice is remembered for one installation of the APK: an update (or a reinstall) tries Vulkan again. A start the
//! system killed half way (a call, the app swiped away) looks like a failure too; that costs this installation Vulkan,
//! which the game hardly needs (it draws one picture and the menus), until the next update.
//!
//! Pure, so the host tests it; the desktop program does not use it.

use crate::launch_report::tr;
use ssd_menus::ui_text::UiText;

/// How far a start got.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// The setup screens' window (the ROM, the mode, the HD pack).
    Setup,
    /// The game loads: the ROM, the packs, the machine.
    Start,
    /// The game's window and its graphics.
    Window,
    /// The sound.
    Audio,
    /// Everything is open; the first picture is being drawn.
    FirstFrame,
}

impl Stage {
    const ALL: [Stage; 5] =
        [Stage::Setup, Stage::Start, Stage::Window, Stage::Audio, Stage::FirstFrame];

    /// The stage's name in the stage file.
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Stage::Setup => "setup",
            Stage::Start => "start",
            Stage::Window => "window",
            Stage::Audio => "audio",
            Stage::FirstFrame => "first-frame",
        }
    }

    fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|stage| stage.code() == code)
    }

    /// The stage in the player's words (a row of the menus' tables).
    fn words(stage: Option<Self>) -> &'static str {
        match stage {
            Some(Stage::Setup) => "the setup screens",
            Some(Stage::Start) => "loading the game",
            Some(Stage::Window) => "opening the game's window and graphics",
            Some(Stage::Audio) => "opening the sound",
            Some(Stage::FirstFrame) => "drawing the first picture",
            None => "an unknown step",
        }
    }
}

/// The graphics backend a start asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// wgpu's order: Vulkan, then OpenGL ES where Vulkan cannot open (`canvas::backends_to_try`).
    Auto,
    /// OpenGL ES alone.
    Gl,
    /// Whatever `WGPU_BACKEND` named before the guard looked (the arguments file of a test build).
    Forced,
}

impl Backend {
    fn code(self) -> &'static str {
        match self {
            Backend::Auto => "auto",
            Backend::Gl => "gl",
            Backend::Forced => "forced",
        }
    }

    fn from_code(code: &str) -> Option<Self> {
        [Backend::Auto, Backend::Gl, Backend::Forced]
            .into_iter()
            .find(|backend| backend.code() == code)
    }
}

/// What the last start left.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Last {
    /// No stage file: it drew its first picture, or there was none before.
    Fine,
    /// A stage file: it died before its first picture. A part the file does not say (or says in a way not known) is
    /// `None`.
    Stopped { stage: Option<Stage>, backend: Option<Backend>, gpu: Option<String> },
}

/// The stage file of a start at `stage` on `backend`, with the graphics adapter once it opened.
#[must_use]
pub fn stage_text(stage: Stage, backend: Backend, gpu: Option<&str>) -> String {
    let mut text = format!("stage={}\nbackend={}\n", stage.code(), backend.code());
    if let Some(gpu) = gpu.filter(|gpu| !gpu.is_empty()) {
        text.push_str("gpu=");
        text.push_str(&gpu.replace(['\n', '\r'], " "));
        text.push('\n');
    }
    text
}

/// What the stage file `text` (`None`: there is none) says of the last start. A file that cannot be read whole still
/// means the start died: its unknown parts are `None`.
#[must_use]
pub fn parse_stage(text: Option<&str>) -> Last {
    let Some(text) = text else { return Last::Fine };
    let value = |key: &str| {
        text.lines()
            .find_map(|line| line.trim().strip_prefix(key)?.strip_prefix('='))
            .map(str::trim)
    };
    Last::Stopped {
        stage: value("stage").and_then(Stage::from_code),
        backend: value("backend").and_then(Backend::from_code),
        gpu: value("gpu").filter(|gpu| !gpu.is_empty()).map(str::to_owned),
    }
}

/// Whether the stage file stays when a run (of the setup screens or of the game) returns before its first picture, at
/// `stage`, having `succeeded` or not. A run that returns on its own did not crash: one that ended as asked (the player
/// quit while it loaded, a benchmark that draws offscreen) or failed before any graphics (a ROM or a pack refused, which
/// its own dialog has explained) says nothing of the backend, and its file goes. One that failed with the graphics open,
/// or in the setup screens (whose failures are their window's), keeps it: the next start counts it as a failure.
#[must_use]
pub fn stage_stays_after_return(stage: Option<Stage>, succeeded: bool) -> bool {
    !succeeded
        && matches!(stage, Some(Stage::Setup | Stage::Window | Stage::Audio | Stage::FirstFrame))
}

/// The file that remembers OpenGL ES for the installation `install`.
#[must_use]
pub fn memory_text(install: &str) -> String {
    format!("backend=gl\ninstall={install}\n")
}

/// Whether the memory file `text` remembers OpenGL ES for the installation `install` (a file of another installation,
/// or one not understood, remembers nothing).
#[must_use]
pub fn remembers_gl(text: Option<&str>, install: &str) -> bool {
    let Some(text) = text else { return false };
    let has = |line: &str| text.lines().any(|l| l.trim() == line);
    has("backend=gl") && has(&format!("install={install}"))
}

/// The key of an installation of the APK, from the path of the game's library and the size and time of its file:
/// every update or reinstall makes another.
#[must_use]
pub fn install_key(library: &str, size: u64, modified: u64) -> String {
    // FNV-1a, 64 bits.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in library.bytes().chain(size.to_le_bytes()).chain(modified.to_le_bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// What the next start is told.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// The last start died on Vulkan (or the automatic order): this one is on OpenGL ES.
    SwitchedToGl(Option<Stage>),
    /// The last start died on OpenGL ES too: neither backend starts the game here.
    BothFailed(Option<Stage>),
    /// The last start died on a backend the arguments file forced: nothing is changed.
    Failed(Option<Stage>),
}

/// What this start does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    /// The backend asked for: `Gl` sets `WGPU_BACKEND=gl`; `Auto` and `Forced` leave the environment as it is.
    pub backend: Backend,
    /// What the player is told before the start; `None`: nothing.
    pub notice: Option<Notice>,
    /// Whether OpenGL ES is remembered for this installation from now on.
    pub remember_gl: bool,
}

/// The decision of a start: `last` is what the last start left, `remembered` whether OpenGL ES is remembered for this
/// installation, `forced` whether `WGPU_BACKEND` was set before the guard (the arguments file), which it never
/// overrides.
#[must_use]
pub fn decide(last: &Last, remembered: bool, forced: bool) -> Decision {
    let usual = if remembered { Backend::Gl } else { Backend::Auto };
    let Last::Stopped { stage, backend, .. } = last else {
        return Decision {
            backend: if forced { Backend::Forced } else { usual },
            notice: None,
            remember_gl: remembered,
        };
    };
    let stage = *stage;
    if forced || *backend == Some(Backend::Forced) {
        // A forced backend says nothing of the automatic one: the start is reported, nothing is switched.
        return Decision {
            backend: if forced { Backend::Forced } else { usual },
            notice: Some(Notice::Failed(stage)),
            remember_gl: remembered,
        };
    }
    // On OpenGL ES already (or not known, with OpenGL ES remembered): both have failed. Stay on OpenGL ES.
    let on_gl = *backend == Some(Backend::Gl) || (backend.is_none() && remembered);
    let notice = if on_gl { Notice::BothFailed(stage) } else { Notice::SwitchedToGl(stage) };
    Decision { backend: Backend::Gl, notice: Some(notice), remember_gl: true }
}

// The English words of the notice: each is a row of the menus' tables (`ui-zh-tw.tsv`, `ui-zh-hk.tsv`).
const TITLE: &str = "The last start failed";
const STOPPED: &str = "The last start stopped before its first picture, at:";
const SWITCHED: &str = "This start uses OpenGL ES instead of Vulkan, and so will the next ones.";
const BOTH: &str = "It was on OpenGL ES, after Vulkan had failed: neither could start the game on this device. This start tries OpenGL ES again.";
const FORCED: &str =
    "The last start's graphics backend was set by the arguments file, so nothing is changed.";

/// Every English text of the notice, so a test can check the tables have them all.
pub const WORDS: &[&str] = &[
    TITLE,
    STOPPED,
    SWITCHED,
    BOTH,
    FORCED,
    "the setup screens",
    "loading the game",
    "opening the game's window and graphics",
    "opening the sound",
    "drawing the first picture",
    "an unknown step",
];

/// The dialog's title for a notice.
#[must_use]
pub fn notice_title(words: Option<&UiText>) -> String {
    tr(words, TITLE)
}

/// The lines that say what happened and what this start does.
#[must_use]
pub fn notice_lines(words: Option<&UiText>, notice: &Notice) -> Vec<String> {
    let (stage, then) = match notice {
        Notice::SwitchedToGl(stage) => (stage, SWITCHED),
        Notice::BothFailed(stage) => (stage, BOTH),
        Notice::Failed(stage) => (stage, FORCED),
    };
    let stopped = tr(words, STOPPED);
    let gap = if stopped.ends_with('：') { "" } else { " " };
    vec![format!("{stopped}{gap}{}", tr(words, Stage::words(*stage))), tr(words, then)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stopped(stage: Stage, backend: Backend) -> Last {
        parse_stage(Some(&stage_text(stage, backend, Some("Adreno (TM) 540 / Vulkan"))))
    }

    /// The stage file after a run returned at `stage` (`succeeded` or not), then the next start's decision.
    fn after_return(stage: Stage, succeeded: bool) -> Decision {
        let file = stage_text(stage, Backend::Auto, None);
        let left = stage_stays_after_return(Some(stage), succeeded).then_some(file);
        decide(&parse_stage(left.as_deref()), false, false)
    }

    #[test]
    fn a_run_that_returned_as_asked_leaves_no_failure_behind() {
        // A benchmark (drawn offscreen, never presented) or the player quitting while the game loads.
        for stage in Stage::ALL {
            assert!(!stage_stays_after_return(Some(stage), true), "{stage:?}");
            assert_eq!(
                after_return(stage, true),
                Decision { backend: Backend::Auto, notice: None, remember_gl: false }
            );
        }
        assert!(!stage_stays_after_return(None, true));
    }

    #[test]
    fn a_run_refused_before_its_graphics_is_not_a_graphics_failure() {
        // The ROM or a pack refused (main.rs's exit codes): reported then, never "both backends failed" later.
        assert!(!stage_stays_after_return(Some(Stage::Start), false));
        assert!(!stage_stays_after_return(None, false));
        assert_eq!(after_return(Stage::Start, false).notice, None);
        assert_eq!(after_return(Stage::Start, false).backend, Backend::Auto);
    }

    #[test]
    fn a_run_that_failed_with_its_graphics_open_still_switches() {
        for stage in [Stage::Setup, Stage::Window, Stage::Audio, Stage::FirstFrame] {
            assert!(stage_stays_after_return(Some(stage), false), "{stage:?}");
            assert_eq!(after_return(stage, false).notice, Some(Notice::SwitchedToGl(Some(stage))));
        }
    }

    #[test]
    fn a_start_that_drew_its_first_picture_changes_nothing() {
        let decision = decide(&parse_stage(None), false, false);
        assert_eq!(decision, Decision { backend: Backend::Auto, notice: None, remember_gl: false });
    }

    #[test]
    fn a_start_that_died_on_vulkan_switches_to_gl_and_remembers_it() {
        let last = stopped(Stage::Window, Backend::Auto);
        assert_eq!(
            last,
            Last::Stopped {
                stage: Some(Stage::Window),
                backend: Some(Backend::Auto),
                gpu: Some("Adreno (TM) 540 / Vulkan".to_owned()),
            }
        );
        let decision = decide(&last, false, false);
        assert_eq!(
            decision,
            Decision {
                backend: Backend::Gl,
                notice: Some(Notice::SwitchedToGl(Some(Stage::Window))),
                remember_gl: true,
            }
        );
    }

    #[test]
    fn gl_remembered_is_used_without_a_word() {
        let install = install_key(
            "/data/app/x/base.apk!/lib/arm64-v8a/libmain.so",
            46_000_000,
            1_700_000_000,
        );
        let memory = memory_text(&install);
        assert!(remembers_gl(Some(&memory), &install));
        let decision = decide(&Last::Fine, remembers_gl(Some(&memory), &install), false);
        assert_eq!(decision, Decision { backend: Backend::Gl, notice: None, remember_gl: true });
    }

    #[test]
    fn another_installation_forgets_gl_and_tries_vulkan_again() {
        let old = install_key("/data/app/a/base.apk!/lib/arm64-v8a/libmain.so", 46_000_000, 1);
        let new = install_key("/data/app/b/base.apk!/lib/arm64-v8a/libmain.so", 46_000_000, 1);
        let same_path_newer =
            install_key("/data/app/a/base.apk!/lib/arm64-v8a/libmain.so", 46_000_000, 2);
        assert_ne!(old, new);
        assert_ne!(old, same_path_newer);
        assert!(!remembers_gl(Some(&memory_text(&old)), &new));
        assert_eq!(decide(&Last::Fine, false, false).backend, Backend::Auto);
    }

    #[test]
    fn a_start_that_died_on_gl_too_says_both_failed_and_stays_on_gl() {
        let decision = decide(&stopped(Stage::FirstFrame, Backend::Gl), true, false);
        assert_eq!(
            decision,
            Decision {
                backend: Backend::Gl,
                notice: Some(Notice::BothFailed(Some(Stage::FirstFrame))),
                remember_gl: true,
            }
        );
        // And again, as long as it dies: never back to Vulkan.
        for _ in 0..3 {
            let again =
                decide(&stopped(Stage::Window, decision.backend), decision.remember_gl, false);
            assert_eq!(again.backend, Backend::Gl);
            assert!(matches!(again.notice, Some(Notice::BothFailed(_))));
        }
    }

    #[test]
    fn a_damaged_or_unknown_stage_file_still_counts_as_a_death() {
        for text in ["", "garbage", "stage=warp\nbackend=dx13\n", "\u{0}\u{1}stage", "backend=auto"]
        {
            let last = parse_stage(Some(text));
            let Last::Stopped { stage, .. } = &last else { panic!("{text:?} is a death") };
            assert_eq!(*stage, None, "{text:?}: no stage known");
            let decision = decide(&last, false, false);
            assert_eq!(decision.backend, Backend::Gl, "{text:?}");
            assert!(matches!(decision.notice, Some(Notice::SwitchedToGl(None))), "{text:?}");
        }
        // Not known which backend, but OpenGL ES remembered: it was OpenGL ES.
        let decision = decide(&parse_stage(Some("stage=audio")), true, false);
        assert_eq!(decision.notice, Some(Notice::BothFailed(Some(Stage::Audio))));
        // A memory file not understood remembers nothing.
        for text in ["", "backend=gl", "install=00\nbackend=vk"] {
            assert!(!remembers_gl(Some(text), "00"), "{text:?}");
        }
        assert!(!remembers_gl(None, "00"));
    }

    #[test]
    fn a_forced_backend_is_never_switched() {
        let now_forced = decide(&stopped(Stage::Window, Backend::Auto), false, true);
        assert_eq!(
            now_forced,
            Decision {
                backend: Backend::Forced,
                notice: Some(Notice::Failed(Some(Stage::Window))),
                remember_gl: false,
            }
        );
        let was_forced = decide(&stopped(Stage::Window, Backend::Forced), false, false);
        assert_eq!(
            was_forced,
            Decision {
                backend: Backend::Auto,
                notice: Some(Notice::Failed(Some(Stage::Window))),
                remember_gl: false,
            }
        );
        assert_eq!(decide(&Last::Fine, true, true).backend, Backend::Forced);
    }

    #[test]
    fn the_stage_file_round_trips_and_keeps_to_one_line_a_value() {
        for stage in Stage::ALL {
            for backend in [Backend::Auto, Backend::Gl, Backend::Forced] {
                let text = stage_text(stage, backend, Some("a\nb"));
                assert_eq!(
                    parse_stage(Some(&text)),
                    Last::Stopped {
                        stage: Some(stage),
                        backend: Some(backend),
                        gpu: Some("a b".to_owned())
                    }
                );
            }
        }
        assert_eq!(
            parse_stage(Some(&stage_text(Stage::Start, Backend::Auto, None))),
            Last::Stopped { stage: Some(Stage::Start), backend: Some(Backend::Auto), gpu: None }
        );
    }

    #[test]
    fn the_notice_says_where_it_stopped_and_what_this_start_does() {
        let lines = notice_lines(None, &Notice::SwitchedToGl(Some(Stage::Window)));
        assert_eq!(
            lines,
            [
                "The last start stopped before its first picture, at: opening the game's window and graphics",
                SWITCHED
            ]
        );
        let table = UiText::parse(include_str!("../../../translations/zh-tw/ui-zh-tw.tsv"));
        let lines = notice_lines(Some(&table), &Notice::BothFailed(None));
        assert!(lines[0].ends_with("：不明的步驟"), "{lines:?}");
        assert!(lines[1].contains("OpenGL ES"), "{lines:?}");
        assert_eq!(notice_title(Some(&table)), "上次啟動失敗");
    }

    #[test]
    fn both_tables_have_every_word_of_the_notice() {
        let kept = ["Vulkan", "OpenGL", "ES"];
        for (name, text) in [
            ("ui-zh-tw.tsv", include_str!("../../../translations/zh-tw/ui-zh-tw.tsv")),
            ("ui-zh-hk.tsv", include_str!("../../../translations/zh-hk/ui-zh-hk.tsv")),
        ] {
            let table = UiText::parse(text);
            for english in WORDS {
                let translated =
                    table.get(english).unwrap_or_else(|| panic!("{name} has no row {english:?}"));
                let mut rest = translated.clone();
                for word in kept {
                    rest = rest.replace(word, "");
                }
                assert!(
                    !rest.chars().any(|c| c.is_ascii_alphabetic()),
                    "{name}: {english:?} is {translated:?}"
                );
            }
        }
    }
}
