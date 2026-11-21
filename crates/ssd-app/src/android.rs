//! The Android entry of the app: `SDL_main`, called by SDL's activity on the thread it starts for the game.
//! Everything else is the desktop program, included from `ssd-app`.
#![cfg(target_os = "android")]
#![allow(unsafe_code)]
#![allow(dead_code)] // `main` is the desktop entry

// The desktop program, as the root of this library: its modules are `crate::...`.
include!("main.rs");

mod android_lifecycle;
mod android_report;
mod android_setup;

use std::ffi::{OsString, c_char, c_int};

/// The file of extra launch arguments, one line, split on spaces, read from the data folder (a test aid:
/// `--mode enhanced`, `--install-hd-pack <zip>`, `--bench-app <replay>...`). Without it the game starts in Classic
/// mode. `ENV:NAME=value` sets an environment variable. A `--bench-app` run (README, "Developer commands") has no
/// standard output here: its report goes to `bench-report.txt` in the data folder, and replays named relatively are
/// looked for there too.
const ARGS_FILE: &str = "android-args.txt";

/// The folder of the game's files: the app's internal folder (private to the app, removed with it; a debug
/// build can be reached with `adb shell run-as`).
fn data_dir() -> Option<PathBuf> {
    // SAFETY: SDL is initialised by the activity before it runs `SDL_main`; the string is static to SDL.
    let path = unsafe { sdl3::sys::system::SDL_GetAndroidInternalStoragePath() };
    if path.is_null() {
        return None;
    }
    // SAFETY: a NUL-terminated string that lives as long as SDL.
    let text = unsafe { std::ffi::CStr::from_ptr(path) }.to_string_lossy().into_owned();
    Some(PathBuf::from(text))
}

/// The files of the Traditional Chinese translations (Taiwan, Hong Kong) the game reads from the data folder
/// (`lang.rs`), packed as the APK's assets from `translations/zh-tw/` and `translations/zh-hk/`
/// (`android/app/build.gradle`); listed in `lang.rs`, whose tests check them.
const TRANSLATION_FILES: &[&str] = lang::APK_TRANSLATION_FILES;

/// The APK's asset listing the other files the game reads from the data folder (`art-override/` read by `hd_art.rs`,
/// `voice/ssj-transform.wav` read by `audio.rs`), one path relative to the data folder a line. `android/build-apk.sh`
/// writes it with the files, copied from the folder `SSD_APK_ASSETS_DIR` names (the desktop launcher's assets).
const BUNDLED_LIST: &str = "bundled-files.txt";

/// The translation's files and those of [`BUNDLED_LIST`].
fn bundled_files() -> Vec<String> {
    let mut names: Vec<String> = TRANSLATION_FILES.iter().map(|name| (*name).to_owned()).collect();
    match asset(BUNDLED_LIST) {
        Some(bytes) => names.extend(
            String::from_utf8_lossy(&bytes).lines().map(str::trim).filter(|line| !line.is_empty()).map(str::to_owned),
        ),
        None => log::warn!("the APK has no asset {BUNDLED_LIST}: no art override or voice is installed"),
    }
    names
}

/// The bytes of the APK's asset `name` (SDL opens a relative path from the assets); `None` when there is none.
fn asset(name: &str) -> Option<Vec<u8>> {
    let path = std::ffi::CString::new(name).ok()?;
    let mut size = 0_usize;
    // SAFETY: a NUL-terminated path and a place for the size; SDL returns a buffer of `size` bytes or null.
    let data = unsafe { sdl3::sys::iostream::SDL_LoadFile(path.as_ptr(), &mut size) };
    if data.is_null() {
        return None;
    }
    // SAFETY: `data` holds `size` bytes until it is freed below, with SDL's own `SDL_free`.
    let bytes = unsafe { std::slice::from_raw_parts(data.cast::<u8>(), size) }.to_vec();
    unsafe { sdl3::sys::stdinc::SDL_free(data) };
    Some(bytes)
}

/// Writes the files of [`bundled_files`] from the APK into the data folder `dir`, where the game looks for them: a
/// file with the same bytes is left, another is replaced through a `.part` file and a rename (a newer APK's files win).
fn install_bundled(dir: &Path) {
    let started = std::time::Instant::now();
    let names = bundled_files();
    let (mut same, mut missing) = (0_usize, 0_usize);
    for name in &names {
        let target = dir.join(name);
        let part = dir.join(format!("{name}.part"));
        // SDL opens a relative path from the data folder first and from the assets only when the folder has no such
        // file (SDL_iostream.c, SDL_IOFromFile): the file installed before is moved aside while the asset is read, or
        // the asset would be that file and a newer APK's file would never be installed.
        let kept = std::fs::rename(&target, &part).is_ok();
        let Some(bytes) = asset(name) else {
            log::warn!("the APK has no asset {name}: it is not installed");
            if kept {
                let _ = std::fs::rename(&part, &target);
            }
            missing += 1;
            continue;
        };
        if kept && std::fs::read(&part).is_ok_and(|old| old == bytes) {
            if let Err(error) = std::fs::rename(&part, &target) {
                log::warn!("{}: {error}", target.display());
            }
            same += 1;
            continue;
        }
        let written = target
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&part, &bytes))
            .and_then(|()| std::fs::rename(&part, &target));
        match written {
            Ok(()) => log::info!("bundled file {} installed", target.display()),
            Err(error) => log::warn!("{}: {error}", target.display()),
        }
    }
    log::info!(
        "bundled files: {} listed, {same} same, {missing} missing, {} ms",
        names.len(),
        started.elapsed().as_millis()
    );
}

fn extra_args(dir: &Path) -> Vec<OsString> {
    std::fs::read_to_string(dir.join(ARGS_FILE))
        .map(|text| text.split_whitespace().map(OsString::from).collect())
        .unwrap_or_default()
}

/// The entry point SDL's activity looks up in `libmain.so`.
#[unsafe(no_mangle)]
pub extern "C" fn SDL_main(_argc: c_int, _argv: *mut *mut c_char) -> c_int {
    android_logger::init_once(
        android_logger::Config::default().with_max_level(log::LevelFilter::Info).with_tag("ssd"),
    );
    forward_stderr();
    // A panic shows its report before the process ends (`android_report`).
    android_report::install_panic_hook();
    let mut base = vec![OsString::from("ssd-recompiled")];
    if let Some(dir) = data_dir() {
        log::info!("data folder {}", dir.display());
        // SAFETY: nothing else has started a thread that reads the environment yet.
        unsafe { std::env::set_var("SSD_DATA_DIR", &dir) };
        // Before the game looks for a translation (`lang.rs` keeps what it found for the whole process), its art
        // overrides (`hd_art.rs`) or the voice (`audio.rs`).
        install_bundled(&dir);
        // Called once a process, before the game reads its PC file; the judgement itself is made once a data folder
        // (its mark, `language_pick::SETTLED_MARK`): a player of an earlier version (who has saves) keeps Taiwan's
        // text and is not asked; a new one meets the first launch's Language page.
        match language_pick::settle_saved_language(&dir) {
            Ok(true) => log::info!(
                "an earlier version's player: the language is Traditional Chinese (Taiwan)"
            ),
            Ok(false) => {}
            Err(error) => {
                log::warn!("the language of an earlier version's player was not settled: {error:#}")
            }
        }
        for arg in extra_args(&dir) {
            // `ENV:NAME=value` sets an environment variable (for example `ENV:WGPU_BACKEND=gl`).
            match arg.to_str().and_then(|a| a.strip_prefix("ENV:")).and_then(|a| a.split_once('=')) {
                // SAFETY: still single-threaded here.
                Some((name, value)) => unsafe { std::env::set_var(name, value) },
                None => base.push(arg),
            }
        }
        if base.iter().any(|arg| arg == "--bench-app") && !base.iter().any(|arg| arg == "--bench-report") {
            base.push(OsString::from("--bench-report"));
            base.push(dir.join("bench-report.txt").into_os_string());
        }
        // The graphics backend of this start, from what the last one left (after the arguments file: a backend it
        // forces is kept), and the dialog when the last start died before its first picture.
        android_report::set_words(lang::interface_text(
            &options::read_settings().unwrap_or_default(),
        ));
        android_report::begin(&dir);
    }
    // One SDL context for the whole life of the activity: the setup screens and the game each open their window
    // from it, and the setup comes back when the game asks for it (Options, or Select + Start in Classic).
    sdl3::hint::set("SDL_AUDIO_DEVICE_STREAM_ROLE", "Game");
    sdl3::hint::set("SDL_ANDROID_LOW_LATENCY_AUDIO", "0");
    let _sdl = sdl3::init();
    let mut forced = None;
    loop {
        let mut args = base.clone();
        if let Some(dir) = data_dir() {
            // The report's words: the language the setup screens and the game show (it may change between runs).
            android_report::set_words(lang::interface_text(
                &options::read_settings().unwrap_or_default(),
            ));
            if !android_setup::run(&mut args, &dir, forced.take()) {
                // The player quit, or the setup screens failed: then their report shows.
                android_report::run_returned(!android_report::has_notes());
                android_report::after_run(false);
                return 0;
            }
            let (mode, pack) = android_setup::game_info(&dir);
            ssd_menus::options::set_game_info(&mode, &pack);
            ssd_menus::options::show_game_page(true);
        }
        log::info!("arguments {args:?}");
        android_report::stage(launch_guard::Stage::Start);
        let code = run(args);
        log::info!("the game ended with {code:?}");
        // The run's stage file goes unless it failed with its graphics open; a run that failed shows why (its error went
        // to stderr, kept by `android_report`).
        android_report::run_returned(code == std::process::ExitCode::SUCCESS);
        android_report::after_run(code != std::process::ExitCode::SUCCESS);
        match android_setup::take_request() {
            Some(request) => forced = Some(request),
            None => return 0,
        }
    }
}

/// Sends what the program writes to stderr (its error messages) to the log, where `adb logcat` shows it.
fn forward_stderr() {
    let mut fds = [0 as c_int; 2];
    // SAFETY: plain POSIX calls on descriptors this function creates; stderr (2) is replaced by the pipe's
    // write end, and the read end belongs to the thread below.
    let reader = unsafe {
        if libc::pipe(fds.as_mut_ptr()) != 0 || libc::dup2(fds[1], 2) < 0 {
            return;
        }
        libc::close(fds[1]);
        fds[0]
    };
    std::thread::spawn(move || {
        use std::io::{BufRead, BufReader};
        use std::os::fd::FromRawFd;
        // SAFETY: `reader` is a descriptor nothing else owns.
        let file = unsafe { std::fs::File::from_raw_fd(reader) };
        for line in BufReader::new(file).lines().map_while(Result::ok) {
            log::error!("{line}");
            android_report::stderr_line(&line);
        }
    });
}
