//! A start that fails on Android shows why: a system dialog with the report of `launch_report` (the build, the device,
//! the graphics adapter, the memory, and what went wrong, scrubbed of paths), also put on the clipboard so the player
//! can paste it into a report. Without it the app closed on a black screen and the reason was only in `adb logcat`,
//! which a player cannot read.
//!
//! Two ways a start fails: the game ends with an error (`run` returns a failure; its message went to stderr, which
//! `android.rs` forwards here line by line, or the setup screens failed) and the dialog shows after it; or the program
//! panics, and the panic hook shows the dialog before the process aborts. Only the first panic of the process reports;
//! a later one (another thread, or a panic while reporting) is only logged, and the dialog waits a bounded time, so a
//! dialog that cannot show never holds the process.
//!
//! A start can also die where nothing reports it (a crash inside a graphics driver). So each start writes how far it got
//! to a stage file in the data folder ([`stage`]), which its first picture deletes ([`first_frame`]); [`begin`] reads
//! what the last start left and lets `launch_guard` decide the graphics backend of this one: on OpenGL ES after a start
//! that died before its first picture, remembered for this installation, with a dialog that says so.
#![cfg(target_os = "android")]
#![allow(unsafe_code)]

use crate::launch_guard::{self, Backend, Last, Stage};
use crate::launch_report::{self, Facts, Headline};
use ssd_menus::ui_text::UiText;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

/// The lines of stderr kept, and the most a report shows.
const STDERR_KEPT: usize = 12;
const STDERR_SHOWN: usize = 6;
/// The longest line of details a report shows (the dialog does not scroll far).
const DETAIL_CHARS: usize = 400;
/// How long a dialog shown from the panic hook may hold the process before it ends anyway.
const PANIC_WAIT: Duration = Duration::from_secs(180);
/// How long the dialog after a failed run may wait for the player.
const ERROR_WAIT: Duration = Duration::from_secs(600);
/// In the data folder: how far the start in progress got (deleted by its first picture).
const STAGE_FILE: &str = "launch-stage.txt";
/// In the data folder: OpenGL ES remembered for an installation.
const MEMORY_FILE: &str = "gpu-backend.txt";
/// In the data folder: the panic of the last start, for the next start's dialog.
const CRASH_FILE: &str = "last-crash.txt";

#[derive(Default)]
struct State {
    /// The menus' table of the player's language (`None`: English).
    words: Option<Arc<UiText>>,
    /// The adapter the window's graphics opened on, as the report names it.
    gpu: String,
    /// Backends that failed before one opened (kept for a report, not a failure by themselves).
    gpu_failures: Vec<String>,
    /// Failures noted by the app (the setup screens), each a reason to report.
    notes: Vec<String>,
    /// The last lines of stderr.
    stderr: VecDeque<String>,
    /// The data folder, where the stage, memory and crash files are.
    dir: Option<PathBuf>,
    /// The backend this process asked for (`begin`).
    backend: Option<Backend>,
    /// The stage of the start in progress, until its first picture.
    stage: Option<Stage>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
/// Set by the first report of the process (a panic, or a failed run): a second one only logs.
static REPORTED: AtomicBool = AtomicBool::new(false);

fn state() -> MutexGuard<'static, Option<State>> {
    STATE.lock().unwrap_or_else(PoisonError::into_inner)
}

fn with<T>(f: impl FnOnce(&mut State) -> T) -> T {
    f(state().get_or_insert_with(State::default))
}

/// As [`with`], for the panic hook: `None` when the state is held (the panic struck while it was), never waiting.
fn try_with<T>(f: impl FnOnce(&mut State) -> T) -> Option<T> {
    let mut guard = match STATE.try_lock() {
        Ok(guard) => guard,
        Err(std::sync::TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
        Err(std::sync::TryLockError::WouldBlock) => return None,
    };
    Some(f(guard.get_or_insert_with(State::default)))
}

/// Reads what the last start left in the data folder `dir` and decides this start's graphics backend
/// (`launch_guard::decide`): sets `WGPU_BACKEND=gl` when it is OpenGL ES, remembers that for this installation, and
/// shows the dialog that says why. Called once, before any window, while the process has no other thread that reads
/// the environment.
pub fn begin(dir: &Path) {
    let read = |name: &str| std::fs::read_to_string(dir.join(name)).ok();
    let forced = std::env::var_os("WGPU_BACKEND").is_some();
    let last = match std::fs::read(dir.join(STAGE_FILE)) {
        Ok(bytes) => launch_guard::parse_stage(Some(&String::from_utf8_lossy(&bytes))),
        Err(_) => Last::Fine,
    };
    let install = install_key();
    let remembered = launch_guard::remembers_gl(read(MEMORY_FILE).as_deref(), &install);
    let decision = launch_guard::decide(&last, remembered, forced);
    log::info!("last start {last:?}, OpenGL ES remembered {remembered}: {decision:?}");
    if decision.remember_gl
        && !remembered
        && let Err(error) =
            std::fs::write(dir.join(MEMORY_FILE), launch_guard::memory_text(&install))
    {
        log::warn!("OpenGL ES was not remembered: {error}");
    }
    if decision.backend == Backend::Gl && !forced {
        // SAFETY: called before any thread that reads the environment starts (`SDL_main`, as its other `set_var`).
        unsafe { std::env::set_var("WGPU_BACKEND", "gl") };
    }
    let _ = std::fs::remove_file(dir.join(STAGE_FILE));
    let crash = read(CRASH_FILE);
    let _ = std::fs::remove_file(dir.join(CRASH_FILE));
    let words = with(|state| {
        state.dir = Some(dir.to_path_buf());
        state.backend = Some(decision.backend);
        state.words.clone()
    });
    if let Some(notice) = &decision.notice {
        let words = words.as_deref();
        let lead = launch_guard::notice_lines(words, notice);
        let gpu = match &last {
            Last::Stopped { gpu, .. } => gpu.clone().unwrap_or_default(),
            Last::Fine => String::new(),
        };
        let details: Vec<String> =
            crash.as_deref().unwrap_or_default().lines().map(str::to_owned).collect();
        let text = launch_report::report(words, &lead, &facts(gpu), &details);
        dialog(&launch_guard::notice_title(words), &text, ERROR_WAIT);
    }
}

/// The start in progress has got to `stage`: the stage file says so (with the backend and the adapter), until the
/// first picture deletes it.
pub fn stage(stage: Stage) {
    let file = with(|state| {
        if matches!(stage, Stage::Setup | Stage::Start) {
            // A run of its own: its adapter is not known yet, and the last run's failures are not its own.
            state.gpu.clear();
            state.gpu_failures.clear();
            state.stderr.clear();
        }
        state.stage = Some(stage);
        stage_file(state)
    });
    write_stage(file);
}

/// The stage file of `state`'s start in progress: where, and what it says.
fn stage_file(state: &State) -> Option<(PathBuf, String)> {
    let (Some(dir), Some(stage)) = (&state.dir, state.stage) else { return None };
    let text =
        launch_guard::stage_text(stage, state.backend.unwrap_or(Backend::Auto), Some(&state.gpu));
    Some((dir.join(STAGE_FILE), text))
}

/// Writes a stage file, outside the state's lock. Not synced: a crash of the process leaves what it wrote to the
/// system's cache, which is all a crash of the game (not of the phone) needs.
/// The stage file is written and deleted on SDL's main thread only: called from another thread, a write that
/// started earlier could land after a later one, and an older stage would overwrite the newer.
fn write_stage(file: Option<(PathBuf, String)>) {
    if let Some((path, text)) = file
        && let Err(error) = std::fs::write(&path, text)
    {
        log::warn!("the stage file was not written: {error}");
    }
}

/// Deletes the stage file of the start in progress, if there is one: the start is over. On SDL's main thread only,
/// as `write_stage`: from another thread an older stage could overwrite the newer.
fn end_stage(why: &str) {
    // the folder is cloned only when a stage was taken (once a start), not on every frame's call
    let ended = with(|state| state.stage.take().and_then(|_| state.dir.clone()));
    if let Some(dir) = ended {
        let _ = std::fs::remove_file(dir.join(STAGE_FILE));
        log::info!("{why}");
    }
}

/// A window presented a picture: the start in progress has worked, and its stage file goes.
pub fn first_frame() {
    // A lock and a check a frame: nothing else is done once the start is over.
    end_stage("the first picture is drawn: the start worked");
}

/// A run (of the setup screens or of the game) returned, `succeeded` or not, before or after its first picture: its
/// stage file goes unless the failure is the graphics' (`launch_guard::stage_stays_after_return`). A run that ended as
/// asked (the player quit while it loaded, a benchmark drawn offscreen) or was refused before any graphics (the ROM, a
/// pack) is not counted against the backend at the next start.
pub fn run_returned(succeeded: bool) {
    let stage = with(|state| state.stage);
    if launch_guard::stage_stays_after_return(stage, succeeded) {
        log::warn!(
            "the run failed at {stage:?}: the next start counts it as the graphics' failure"
        );
    } else {
        end_stage("the run returned: its stage file goes");
    }
}

/// Whether the app noted a failure since the last report.
pub fn has_notes() -> bool {
    with(|state| !state.notes.is_empty())
}

/// The key of this installation of the APK: the path of the game's library (`dladdr`) and the size and time of the
/// file that holds it (the APK, or the library unpacked beside it).
fn install_key() -> String {
    // SAFETY: `Dl_info` is plain data, for which all zeroes is a valid value.
    let mut info: libc::Dl_info = unsafe { std::mem::zeroed() };
    // SAFETY: `dladdr` looks up an address of this library and fills `info`; its name lives as long as the library.
    let found = unsafe { libc::dladdr(install_key as *const std::ffi::c_void, &raw mut info) } != 0;
    let library = if found && !info.dli_fname.is_null() {
        // SAFETY: a NUL-terminated name from `dladdr`.
        unsafe { std::ffi::CStr::from_ptr(info.dli_fname) }.to_string_lossy().into_owned()
    } else {
        String::new()
    };
    let file = library.split_once('!').map_or(library.as_str(), |(apk, _)| apk);
    let (size, modified) = std::fs::metadata(file).map_or((0, 0), |meta| {
        let modified = meta
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |since| since.as_secs());
        (meta.len(), modified)
    });
    launch_guard::install_key(&library, size, modified)
}

/// The words of the dialog from now on: the menus' table of the language the player chose (as the setup screens have
/// it). Before this is called (a panic very early) the dialog is in English.
pub fn set_words(words: Option<Arc<UiText>>) {
    with(|state| state.words = words);
}

/// The graphics opened on this adapter (the stage file names it, so a crash in its driver is reported with it).
pub fn gpu_opened(text: String) {
    let file = with(|state| {
        state.gpu = text;
        stage_file(state)
    });
    write_stage(file);
}

/// A graphics backend that failed before the next was tried.
pub fn gpu_failed(text: String) {
    with(|state| state.gpu_failures.push(text));
}

/// A failure the app noted itself: the run is reported even if its exit code says nothing.
pub fn note(text: String) {
    with(|state| state.notes.push(text));
}

/// A line the program wrote to stderr (`android.rs` forwards them).
pub fn stderr_line(line: &str) {
    with(|state| {
        if state.stderr.len() == STDERR_KEPT {
            state.stderr.pop_front();
        }
        state.stderr.push_back(line.to_owned());
    });
}

/// After a run of the game (or of the setup screens): when it `failed` or noted a failure, shows the report and
/// returns `true`.
pub fn after_run(failed: bool) -> bool {
    if !failed && with(|state| state.notes.is_empty()) {
        return false;
    }
    // The stderr thread may still be passing on the last lines.
    std::thread::sleep(Duration::from_millis(200));
    if REPORTED.swap(true, Ordering::SeqCst) {
        return false;
    }
    let (words, gpu, details) = with(|state| {
        let notes = std::mem::take(&mut state.notes);
        let stderr: Vec<String> = state.stderr.drain(..).collect();
        let mut details = state.gpu_failures.clone();
        // The app's own notes say what failed; without one (an exit code with its reason printed), the last lines of
        // stderr do (with whatever a driver printed there too).
        if notes.is_empty() {
            details.extend(stderr.iter().rev().take(STDERR_SHOWN).rev().cloned());
        } else {
            details.extend(notes);
        }
        (state.words.clone(), state.gpu.clone(), details)
    });
    show(words.as_deref(), gpu, Headline::CouldNotStart, &details, ERROR_WAIT);
    // A later run of the same process (the launch loop) reports again.
    REPORTED.store(false, Ordering::SeqCst);
    true
}

/// The panic hook: logs the panic, then (the first panic only) shows the report and ends the process, whatever thread
/// panicked: a worker's panic would otherwise leave the game running under a dialog that says it stopped, and every
/// later failure unreported. A later panic (another thread, while the first one's dialog shows) is only logged.
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        log::error!("panic: {info}");
        if REPORTED.swap(true, Ordering::SeqCst) {
            return;
        }
        let thread = std::thread::current();
        let details = vec![format!("panic ({}): {info}", thread.name().unwrap_or("SDL"))];
        // Never waits for the state: the panic may have struck while it was held.
        let (words, gpu, dir, stage) = try_with(|state| {
            (state.words.clone(), state.gpu.clone(), state.dir.clone(), state.stage)
        })
        .unwrap_or_default();
        if let Some(dir) = dir {
            // For the next start's dialog, in case this one cannot show.
            let scrubbed: Vec<String> =
                details.iter().map(|line| launch_report::scrub(line)).collect();
            let _ = std::fs::write(dir.join(CRASH_FILE), scrubbed.join("\n"));
            // As a run that returned with a failure: a panic before any graphics (or after the first picture, when
            // there is no stage file) does not switch the next start to OpenGL ES.
            if !launch_guard::stage_stays_after_return(stage, false) {
                let _ = std::fs::remove_file(dir.join(STAGE_FILE));
            }
        }
        show(words.as_deref(), gpu, Headline::Panicked, &details, PANIC_WAIT);
        std::process::abort();
    }));
}

/// The facts of this device and build, with the graphics adapter `gpu`.
pub fn facts(gpu: String) -> Facts {
    let chip = [property("ro.soc.model"), property("ro.board.platform"), property("ro.hardware")]
        .into_iter()
        .find(|value| !value.trim().is_empty())
        .unwrap_or_default();
    Facts {
        version: option_env!("SSD_APK_VERSION").unwrap_or_default().to_owned(),
        manufacturer: property("ro.product.manufacturer"),
        model: property("ro.product.model"),
        android: property("ro.build.version.release"),
        api: property("ro.build.version.sdk"),
        chip,
        gpu,
        memory_mb: memory_mb(),
    }
}

/// Shows the report for `headline` with `details` (scrubbed here), and puts it on the clipboard.
fn show(
    words: Option<&UiText>,
    gpu: String,
    headline: Headline,
    details: &[String],
    wait: Duration,
) {
    let details: Vec<String> = details
        .iter()
        .map(|line| {
            let line = launch_report::scrub(line);
            match line.char_indices().nth(DETAIL_CHARS) {
                Some((at, _)) => format!("{}…", &line[..at]),
                None => line,
            }
        })
        .collect();
    let lead = [launch_report::headline(words, headline)];
    let text = launch_report::report(words, &lead, &facts(gpu), &details);
    dialog(&launch_report::title(words, headline), &text, wait);
}

/// Puts `text` on the clipboard and shows it in a system dialog under `title`, waiting at most `wait` for the player to
/// close it. The dialog runs on a thread of its own, so a dialog that cannot show (the activity's thread held) costs
/// the wait, not the process.
pub fn dialog(title: &str, text: &str, wait: Duration) {
    use sdl3::sys::init::{SDL_INIT_VIDEO, SDL_InitSubSystem, SDL_QuitSubSystem};
    log::warn!("report: {title}\n{text}");
    // The clipboard is the video subsystem's: held for the copy (SDL counts the holds, so a window's is not ended).
    // SAFETY: plain SDL calls; the hold taken is given back below.
    let video = unsafe { SDL_InitSubSystem(SDL_INIT_VIDEO) };
    let clip = std::ffi::CString::new(text.replace('\0', " ")).unwrap_or_default();
    // SAFETY: a NUL-terminated string, which SDL copies.
    if !unsafe { sdl3::sys::clipboard::SDL_SetClipboardText(clip.as_ptr()) } {
        log::warn!("the report was not put on the clipboard: {}", sdl3::get_error());
    }
    if video {
        // SAFETY: the hold taken above.
        unsafe { SDL_QuitSubSystem(SDL_INIT_VIDEO) };
    }
    let (done, finished) = std::sync::mpsc::channel();
    let (title, text) = (title.to_owned(), text.to_owned());
    let shown = std::thread::Builder::new().name("report dialog".to_owned()).spawn(move || {
        if let Err(error) = sdl3::messagebox::show_simple_message_box(
            sdl3::messagebox::MessageBoxFlag::ERROR,
            &title,
            &text,
            None,
        ) {
            log::warn!("the report dialog did not show: {error}");
        }
        let _ = done.send(());
    });
    if shown.is_ok() && finished.recv_timeout(wait).is_err() {
        log::warn!("the report dialog was not closed in {} s: going on", wait.as_secs());
    }
}

/// The system property `name` (empty when it has none).
pub fn property(name: &str) -> String {
    let Ok(name) = std::ffi::CString::new(name) else { return String::new() };
    let mut value = [0 as std::ffi::c_char; 92]; // PROP_VALUE_MAX
    // SAFETY: `name` is NUL-terminated and `value` has the room the call writes (PROP_VALUE_MAX bytes).
    if unsafe { libc::__system_property_get(name.as_ptr(), value.as_mut_ptr()) } <= 0 {
        return String::new();
    }
    // SAFETY: the call wrote a NUL-terminated value into `value`, which is zeroed beyond it.
    unsafe { std::ffi::CStr::from_ptr(value.as_ptr()) }.to_string_lossy().trim().to_owned()
}

/// The memory in megabytes, total and available, from `/proc/meminfo`.
fn memory_mb() -> Option<(u64, u64)> {
    let text = std::fs::read_to_string("/proc/meminfo").ok()?;
    let field = |name: &str| {
        text.lines()
            .find_map(|line| line.strip_prefix(name))
            .and_then(|rest| rest.trim().trim_end_matches("kB").trim().parse::<u64>().ok())
            .map(|kib| kib / 1024)
    };
    Some((field("MemTotal:")?, field("MemAvailable:")?))
}

/// Test builds only (`debug_assertions`: never compiled into a release APK): `SSD_TEST_FAIL=panic`, `abort`, `quit` (the
/// quit event SDL sends when the activity closes) or `thread-panic` (a worker thread that panics three seconds later,
/// after the first picture), set
/// through the arguments file (`ENV:SSD_TEST_FAIL=abort`, which only a debuggable build lets `adb` write), fails the
/// start here, after the game's window opened and before its first picture, so the report can be tried on an emulator.
/// `abort` ends the process as a crash in a driver would, without the panic hook.
#[cfg(debug_assertions)]
pub fn test_failure() {
    match std::env::var("SSD_TEST_FAIL").as_deref() {
        Ok("panic") => panic!("SSD_TEST_FAIL=panic: a panic before the first frame"),
        Ok("quit") => {
            // The activity closed while the game loads: SDL's quit event, before the first picture.
            log::warn!("SSD_TEST_FAIL=quit: the game is asked to quit before the first frame");
            // SAFETY: `SDL_Event` is a plain C union, for which all zeroes is a valid value.
            let mut event: sdl3::sys::events::SDL_Event = unsafe { std::mem::zeroed() };
            event.r#type = sdl3::sys::events::SDL_EVENT_QUIT.into();
            // SAFETY: a quit event, which SDL copies into its queue.
            unsafe { sdl3::sys::events::SDL_PushEvent(&raw mut event) };
        }
        Ok("thread-panic") => {
            // A worker that panics after the first picture: the process must end, and the next start not switch.
            let _ = std::thread::Builder::new().name("test worker".to_owned()).spawn(|| {
                std::thread::sleep(Duration::from_secs(3));
                panic!("SSD_TEST_FAIL=thread-panic: a worker thread panics");
            });
        }
        Ok("abort") => {
            log::warn!("SSD_TEST_FAIL=abort: the process ends before the first frame");
            std::process::abort();
        }
        _ => {}
    }
}
