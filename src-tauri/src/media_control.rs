//! Cross-platform system media playback control for the "Pause System Audio" feature.
//!
//! # Strategy per platform
//! - **macOS**  — `MRMediaRemoteSendCommand` (private MediaRemote.framework) pauses the
//!                system-wide "Now Playing" session, which covers any app integrated with
//!                macOS media controls (music players, browsers, podcast apps). Playback
//!                detection parses coreaudiod's `audio-out` entries in `pmset -g
//!                assertions` because since macOS 15.4 the MediaRemote *query* APIs
//!                return nothing for non-entitled processes, while sending commands
//!                still works (verified on macOS 26.5). An audio-out assertion does
//!                not prove a *playing* Now Playing session exists (WebAudio pages,
//!                games and calls hold one too, and a user-paused session holds
//!                none), while Play is the one dangerous command: without a session
//!                macOS launches Music.app, and with a session the user paused it
//!                overrides their choice. So after sending Pause a watcher thread
//!                records which holders release their assertion within ~3 s — only
//!                those were demonstrably paused by us, and only their presence
//!                allows resume to send Play.
//! - **Windows** — WinRT (PowerShell) to detect playback state + Win32 `keybd_event` FFI
//!                 to send `VK_MEDIA_PLAY_PAUSE` (0xB3)
//! - **Linux**  — MPRIS2 over D-Bus via `dbus-send` (present on every major desktop),
//!                with `playerctl` as fallback
//!
//! `pause_system_media()` returns a list of identifiers for what it paused.
//! Pass that list to `resume_system_media()` so we only resume what *we* paused.

/// Pauses currently playing system media.
///
/// Returns a list of opaque identifiers representing what was paused.
/// Pass this list to `resume_system_media` when recording stops.
/// An empty list means nothing was playing (or detection failed).
pub fn pause_system_media() -> Vec<String> {
    platform_pause()
}

/// Resumes media that was previously paused by `pause_system_media`.
///
/// Passing an empty slice is a no-op.
pub fn resume_system_media(paused: &[String]) {
    if paused.is_empty() {
        return;
    }
    platform_resume(paused);
}

/// Ignore audio-out assertions younger than this. Notification "dings" and UI
/// sounds hold one only for the sound's length plus ~2 s of release lag
/// (measured with afplay on macOS 26.5), while real playback accumulates
/// minutes; a ding coinciding with recording start must not count as music.
#[cfg(target_os = "macos")]
const MIN_PLAYBACK_AGE_SECS: u64 = 5;

/// One coreaudiod power assertion with `Resources: audio-out`: the client
/// process it was created for and how long it has existed.
#[cfg(target_os = "macos")]
#[derive(Debug, PartialEq)]
struct AudioOutAssertion {
    pid: u32,
    age_secs: u64,
}

#[cfg(target_os = "macos")]
fn macos_audio_out_assertions() -> Vec<AudioOutAssertion> {
    match std::process::Command::new("pmset")
        .args(["-g", "assertions"])
        .output()
    {
        Ok(out) => parse_audio_out_assertions(&String::from_utf8_lossy(&out.stdout)),
        Err(_) => Vec::new(),
    }
}

/// Parses `pmset -g assertions` output. Assertion rows look like
/// `   pid 414(coreaudiod): [0x…] 00:01:33 PreventUserIdleSystemSleep named: "…"`,
/// followed by indented detail rows, of which two matter here:
/// `Created for PID: 1006.` and `Resources: audio-out …`.
#[cfg(target_os = "macos")]
fn parse_audio_out_assertions(pmset_output: &str) -> Vec<AudioOutAssertion> {
    fn age_secs(row: &str) -> Option<u64> {
        row.split_whitespace().find_map(|w| {
            let b = w.as_bytes();
            if b.len() == 8 && b[2] == b':' && b[5] == b':' {
                Some(
                    w[0..2].parse::<u64>().ok()? * 3600
                        + w[3..5].parse::<u64>().ok()? * 60
                        + w[6..8].parse::<u64>().ok()?,
                )
            } else {
                None
            }
        })
    }

    struct Pending {
        age_secs: u64,
        pid: Option<u32>,
        audio_out: bool,
    }
    fn flush(found: &mut Vec<AudioOutAssertion>, pending: Option<Pending>) {
        if let Some(p) = pending {
            if p.audio_out {
                if let Some(pid) = p.pid {
                    found.push(AudioOutAssertion {
                        pid,
                        age_secs: p.age_secs,
                    });
                }
            }
        }
    }

    let mut found = Vec::new();
    let mut pending: Option<Pending> = None;
    for line in pmset_output.lines() {
        let row = line.trim_start();
        if row.starts_with("pid ") && row.contains("): [") {
            flush(&mut found, pending.take());
            if row.contains("(coreaudiod):") {
                pending = Some(Pending {
                    // Unparseable age counts as old: better to pause real music
                    // than to skip it, and resume re-verifies anyway.
                    age_secs: age_secs(row).unwrap_or(u64::MAX),
                    pid: None,
                    audio_out: false,
                });
            }
        } else if let Some(p) = pending.as_mut() {
            if let Some(rest) = row.strip_prefix("Created for PID:") {
                p.pid = rest.trim().trim_end_matches('.').parse().ok();
            } else if row.starts_with("Resources:") && row.contains("audio-out") {
                p.audio_out = true;
            }
        }
    }
    flush(&mut found, pending.take());
    found
}

/// Outcome of the post-Pause verification, shared between the pause-time
/// watcher thread and the resume-time thread. `confirmed: None` means the
/// watcher is still running; `Some(pids)` lists the holders that released
/// their audio-out assertion shortly after our Pause — i.e. the processes the
/// Pause demonstrably paused. The generation ties a verdict to one
/// pause/resume cycle so a stale watcher cannot feed a later recording.
#[cfg(target_os = "macos")]
struct MacosPauseVerdict {
    generation: u64,
    confirmed: Option<Vec<u32>>,
}

#[cfg(target_os = "macos")]
static MACOS_PAUSE_VERDICT: std::sync::Mutex<MacosPauseVerdict> =
    std::sync::Mutex::new(MacosPauseVerdict {
        generation: 0,
        confirmed: Some(Vec::new()),
    });

/// How long after sending Pause a genuinely paused player has to release its
/// audio-out assertion. Measured release lag is ~2 s; holders still streaming
/// past this window were not paused by us (WebAudio pages, games, calls).
#[cfg(target_os = "macos")]
const PAUSE_VERIFY_WINDOW_MS: u64 = 3000;

#[cfg(target_os = "macos")]
fn platform_pause() -> Vec<String> {
    // An audio-out assertion is necessary but not sufficient for "music is
    // playing": WebAudio pages, games and calls hold one without registering a
    // Now Playing session, and those MRMediaRemoteSendCommand cannot pause.
    // Filter what we can here (our own process, short-lived notification
    // sounds), then send Pause — which is always safe: on an already-paused or
    // absent Now Playing session it is a no-op. Play is the dangerous command
    // (it resumes sessions the *user* paused, or launches Music.app when no
    // session exists), so whether resume may send it is decided by watching
    // which holders actually stop streaming right after this Pause.
    let own_pid = std::process::id();
    let holders: Vec<u32> = macos_audio_out_assertions()
        .into_iter()
        .filter(|a| a.pid != own_pid && a.age_secs >= MIN_PLAYBACK_AGE_SECS)
        .map(|a| a.pid)
        .collect();
    if holders.is_empty() {
        return Vec::new();
    }
    let _ = media_remote::send_command(media_remote::Command::Pause);

    let generation = {
        let mut verdict = MACOS_PAUSE_VERDICT
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        verdict.generation += 1;
        verdict.confirmed = None;
        verdict.generation
    };
    // The caller holds the audio-state lock and recording must start without
    // delay, so the verification runs on a detached watcher thread.
    std::thread::spawn(move || {
        let mut confirmed: Vec<u32> = Vec::new();
        let rounds = PAUSE_VERIFY_WINDOW_MS / 500;
        for _ in 0..rounds {
            std::thread::sleep(std::time::Duration::from_millis(500));
            let streaming = macos_audio_out_assertions();
            for pid in &holders {
                if !confirmed.contains(pid) && !streaming.iter().any(|a| a.pid == *pid) {
                    confirmed.push(*pid);
                }
            }
            if confirmed.len() == holders.len() {
                break;
            }
        }
        let mut verdict = MACOS_PAUSE_VERDICT
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if verdict.generation == generation {
            verdict.confirmed = Some(confirmed);
        }
    });

    vec![format!("__macos_media__:{generation}")]
}

#[cfg(target_os = "macos")]
fn platform_resume(paused_apps: &[String]) {
    let generation: u64 = match paused_apps
        .iter()
        .find_map(|t| t.strip_prefix("__macos_media__:")?.parse().ok())
    {
        Some(g) => g,
        None => return,
    };
    // Callers hold the audio-state lock; waiting for the watcher verdict can
    // take up to the verification window, so run detached.
    std::thread::spawn(move || {
        // Wait for the pause-time watcher to finish (recordings shorter than
        // the verification window stop before it has a verdict).
        let deadline = std::time::Instant::now()
            + std::time::Duration::from_millis(PAUSE_VERIFY_WINDOW_MS + 1500);
        let confirmed: Vec<u32> = loop {
            {
                let verdict = MACOS_PAUSE_VERDICT
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                if verdict.generation != generation {
                    // A newer pause cycle superseded this one; let it decide.
                    return;
                }
                if let Some(c) = &verdict.confirmed {
                    break c.clone();
                }
            }
            if std::time::Instant::now() >= deadline {
                eprintln!("[media_control] skipping resume: pause verification never finished");
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(150));
        };

        if confirmed.is_empty() {
            // Our Pause did not stop any audio stream, so there was no playing
            // Now Playing session: either none exists (Play would launch
            // Music.app) or the user paused it themselves before recording
            // (Play would override their choice). Either way, stay silent.
            eprintln!(
                "[media_control] skipping resume: the pause didn't stop any audio stream \
                 (background audio without a Now Playing session?)"
            );
            return;
        }

        let streaming = macos_audio_out_assertions();
        if confirmed
            .iter()
            .any(|pid| streaming.iter().any(|a| a.pid == *pid))
        {
            // The paused app is emitting audio again: the user resumed it
            // mid-recording, so playback is already where they want it.
            return;
        }

        // If every confirmed app exited during the recording, a Play would hit
        // no session and launch Music.app.
        let any_alive = confirmed.iter().any(|pid| {
            std::process::Command::new("kill")
                .args(["-0", &pid.to_string()])
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
        });
        if !any_alive {
            eprintln!("[media_control] skipping resume: paused app(s) no longer running");
            return;
        }

        let _ = media_remote::send_command(media_remote::Command::Play);
    });
}

#[cfg(target_os = "windows")]
fn platform_pause() -> Vec<String> {
    if windows_is_media_playing() {
        windows_send_media_play_pause();
        vec!["__windows_media__".to_string()]
    } else {
        Vec::new()
    }
}

#[cfg(target_os = "windows")]
fn platform_resume(paused: &[String]) {
    if paused.iter().any(|p| p == "__windows_media__") {
        windows_send_media_play_pause();
    }
}

/// Uses WinRT (GlobalSystemMediaTransportControls) + single PowerShell call
/// to detect if any media is playing. Much faster than previous per-process loop.
#[cfg(target_os = "windows")]
fn windows_is_media_playing() -> bool {
    // Primary: WinRT GlobalSystemMediaTransportControls (Windows 10 1903+)
    let winrt_script = r#"
try {
    $null = [Windows.Media.Control.GlobalSystemMediaTransportControlsSessionManager,Windows.Media,ContentType=WindowsRuntime]
    $methods = [System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object {
        $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1
    }
    if ($methods.Count -eq 0) { throw 'AsTask not found' }
    $asTask = $methods[0]
    $reqOp = [Windows.Media.Control.GlobalSystemMediaTransportControlsSessionManager]::RequestAsync()
    $task = $asTask.MakeGenericMethod([Windows.Media.Control.GlobalSystemMediaTransportControlsSessionManager]).Invoke($null, @($reqOp))
    $mgr = $task.GetAwaiter().GetResult()
    $sess = $mgr.GetCurrentSession()
    if ($null -ne $sess) {
        $status = $sess.GetPlaybackInfo().PlaybackStatus.ToString()
        if ($status -eq 'Playing') { Write-Output '1'; exit 0 }
    }
} catch {}
Write-Output '0'
"#;

    let winrt_result = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
            winrt_script,
        ])
        .output();

    if let Ok(output) = winrt_result {
        let text = String::from_utf8_lossy(&output.stdout);
        if text.trim() == "1" {
            return true;
        }
        // If we got a definitive "0" answer (WinRT worked but nothing playing) → not playing
        if text.trim() == "0" {
            return false;
        }
    }

    // Fallback: single PowerShell call to check multiple known media players
    let known_procs = [
        "Spotify", "vlc", "wmplayer", "groove", "msedge", "chrome", "firefox",
        "foobar2000", "winamp", "musicbee", "aimp", "iTunes", "AppleMusic",
    ];

    let proc_list = known_procs.join(",");
    let fallback_script = format!(
        "Get-Process -Name {} -ErrorAction SilentlyContinue | Select-Object -First 1 -ExpandProperty Name",
        proc_list
    );

    let fallback_result = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
            &fallback_script,
        ])
        .output();

    if let Ok(output) = fallback_result {
        let text = String::from_utf8_lossy(&output.stdout).trim().to_lowercase();
        if !text.is_empty() {
            return true; // any of the known media players is running
        }
    }

    false
}

/// Sends a `VK_MEDIA_PLAY_PAUSE` (0xB3) key-down + key-up via Win32 `keybd_event`.
/// `user32.dll` is always available on Windows; no extra crate needed.
#[cfg(target_os = "windows")]
fn windows_send_media_play_pause() {
    #[link(name = "user32")]
    extern "system" {
        fn keybd_event(bvk: u8, bscan: u8, dw_flags: u32, dw_extra_info: usize);
    }

    const VK_MEDIA_PLAY_PAUSE: u8 = 0xB3;
    const KEYEVENTF_KEYUP: u32 = 0x0002;

    unsafe {
        keybd_event(VK_MEDIA_PLAY_PAUSE, 0, 0, 0); // key down
        std::thread::sleep(std::time::Duration::from_millis(15));
        keybd_event(VK_MEDIA_PLAY_PAUSE, 0, KEYEVENTF_KEYUP, 0); // key up
    }
}

#[cfg(target_os = "linux")]
fn platform_pause() -> Vec<String> {
    // Primary: MPRIS2 via dbus-send
    // dbus-send ships with every major desktop environment (GNOME, KDE, XFCE,
    // LXQt, MATE, Cinnamon …) and is available on Debian, Ubuntu, Fedora,
    // Arch, openSUSE, Alpine, Void, Gentoo, etc.
    if let Some(paused) = linux_mpris_pause() {
        if !paused.is_empty() {
            return paused;
        }
    }

    // Fallback: playerctl
    // playerctl is optional but widely packaged. It works as a high-level
    // wrapper around MPRIS2 and covers edge cases dbus-send misses.
    linux_playerctl_pause()
}

#[cfg(target_os = "linux")]
fn platform_resume(paused: &[String]) {
    for player in paused {
        if player.starts_with("org.mpris.MediaPlayer2.") {
            linux_mpris_play(player);
        } else if player == "__playerctl__" {
            let _ = std::process::Command::new("playerctl").arg("play").status();
        }
    }
}

/// Lists MPRIS2 players via `dbus-send`, checks each for `PlaybackStatus == Playing`,
/// pauses those that are, and returns their D-Bus service names.
#[cfg(target_os = "linux")]
fn linux_mpris_pause() -> Option<Vec<String>> {
    // List all names on the session bus
    let list_out = std::process::Command::new("dbus-send")
        .args([
            "--session",
            "--dest=org.freedesktop.DBus",
            "--type=method_call",
            "--print-reply",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus.ListNames",
        ])
        .output()
        .ok()?;

    if !list_out.status.success() {
        return None;
    }

    let names_str = String::from_utf8_lossy(&list_out.stdout);

    // Collect MPRIS player service names
    let players: Vec<String> = names_str
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim().trim_matches('"');
            if trimmed.starts_with("org.mpris.MediaPlayer2.") {
                Some(trimmed.to_string())
            } else {
                None
            }
        })
        .collect();

    let mut paused = Vec::new();

    for player in &players {
        // Query PlaybackStatus property
        let status_out = std::process::Command::new("dbus-send")
            .args([
                "--session",
                &format!("--dest={}", player),
                "--type=method_call",
                "--print-reply",
                "/org/mpris/MediaPlayer2",
                "org.freedesktop.DBus.Properties.Get",
                "string:org.mpris.MediaPlayer2.Player",
                "string:PlaybackStatus",
            ])
            .output();

        let is_playing = match status_out {
            Ok(o) => String::from_utf8_lossy(&o.stdout).contains("\"Playing\""),
            Err(_) => false,
        };

        if is_playing {
            let pause_ok = std::process::Command::new("dbus-send")
                .args([
                    "--session",
                    &format!("--dest={}", player),
                    "--type=method_call",
                    "/org/mpris/MediaPlayer2",
                    "org.mpris.MediaPlayer2.Player.Pause",
                ])
                .status()
                .map(|s| s.success())
                .unwrap_or(false);

            if pause_ok {
                paused.push(player.clone());
            }
        }
    }

    Some(paused)
}

/// Resumes a specific MPRIS2 player using `dbus-send`.
#[cfg(target_os = "linux")]
fn linux_mpris_play(player: &str) {
    let _ = std::process::Command::new("dbus-send")
        .args([
            "--session",
            &format!("--dest={}", player),
            "--type=method_call",
            "/org/mpris/MediaPlayer2",
            "org.mpris.MediaPlayer2.Player.Play",
        ])
        .status();
}

/// Fallback: use `playerctl` to pause if it is installed.
#[cfg(target_os = "linux")]
fn linux_playerctl_pause() -> Vec<String> {
    // Check if playerctl is available
    if std::process::Command::new("playerctl")
        .arg("--version")
        .output()
        .is_err()
    {
        return Vec::new();
    }

    // Check if anything is currently playing
    let status = std::process::Command::new("playerctl")
        .arg("status")
        .output();

    match status {
        Ok(o) if String::from_utf8_lossy(&o.stdout).trim() == "Playing" => {
            let ok = std::process::Command::new("playerctl")
                .arg("pause")
                .status()
                .map(|s| s.success())
                .unwrap_or(false);

            if ok {
                vec!["__playerctl__".to_string()]
            } else {
                Vec::new()
            }
        }
        _ => Vec::new(),
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
fn platform_pause() -> Vec<String> {
    Vec::new()
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
fn platform_resume(_paused: &[String]) {}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    // Captured verbatim from `pmset -g assertions` on macOS 26.5 (2026-06-11):
    // Zen Browser (pid 1006) holding an audio-out assertion for 1 min 33 s.
    const PMSET_ZEN_PLAYING: &str = r#"2026-06-11 13:02:17 +0200
Assertion status system-wide:
   BackgroundTask                 0
   UserIsActive                   1
   PreventUserIdleSystemSleep     1
Listed by owning process:
   pid 72128(caffeinate): [0x0001d5c70001a60e] 00:03:59 PreventUserIdleSystemSleep named: "caffeinate command-line tool"
	Details: caffeinate asserting for 300 secs
	Localized=THE CAFFEINATE TOOL IS PREVENTING SLEEP.
	Timeout will fire in 61 secs Action=TimeoutActionRelease
   pid 404(WindowServer): [0x0001c39d00099c69] 00:01:33 UserIsActive named: "com.apple.iohideventsystem.queue.tickle serviceID:1000009f6"
	Timeout will fire in 1107 secs Action=TimeoutActionRelease
   pid 345(powerd): [0x0001beea00019ae1] 01:41:31 PreventUserIdleSystemSleep named: "Powerd - Prevent sleep while display is on"
   pid 414(coreaudiod): [0x0001d6590001a731] 00:01:33 PreventUserIdleSystemSleep named: "com.apple.audio.30-7A-D2-65-82-4A:output.context.preventuseridlesleep"
	Created for PID: 1006.
	Resources: audio-out 30-7A-D2-65-82-4A:output
   pid 680(sharingd): [0x0001d6400001a743] 00:01:57 PreventUserIdleSystemSleep named: "Handoff"
No kernel assertions."#;

    // afplay (pid 73379) a fraction of a second into a notification-style sound;
    // coreaudiod entry is the last assertion in the listing (exercises the
    // end-of-input flush).
    const PMSET_FRESH_DING_LAST: &str = r#"Listed by owning process:
   pid 345(powerd): [0x0001beea00019ae1] 01:44:23 PreventUserIdleSystemSleep named: "Powerd - Prevent sleep while display is on"
   pid 414(coreaudiod): [0x0001d74c0001a784] 00:00:00 PreventUserIdleSystemSleep named: "com.apple.audio.BuiltInSpeakerDevice.context.preventuseridlesleep"
	Created for PID: 73379.
	Resources: audio-out BuiltInSpeakerDevice
No kernel assertions."#;

    const PMSET_MIC_ONLY: &str = r#"Listed by owning process:
   pid 414(coreaudiod): [0x0001d74c0001a784] 00:00:42 PreventUserIdleSystemSleep named: "com.apple.audio.BuiltInMicrophoneDevice.context.preventuseridlesleep"
	Created for PID: 555.
	Resources: audio-in BuiltInMicrophoneDevice
No kernel assertions."#;

    #[test]
    fn parses_audio_out_holder_with_age() {
        assert_eq!(
            parse_audio_out_assertions(PMSET_ZEN_PLAYING),
            vec![AudioOutAssertion {
                pid: 1006,
                age_secs: 93
            }]
        );
    }

    #[test]
    fn parses_trailing_assertion_without_following_row() {
        assert_eq!(
            parse_audio_out_assertions(PMSET_FRESH_DING_LAST),
            vec![AudioOutAssertion {
                pid: 73379,
                age_secs: 0
            }]
        );
    }

    #[test]
    fn ignores_audio_in_assertions() {
        assert_eq!(parse_audio_out_assertions(PMSET_MIC_ONLY), Vec::new());
    }

    #[test]
    fn no_coreaudiod_means_nothing_playing() {
        let out = "Listed by owning process:\n   pid 345(powerd): [0x01] 01:41:31 PreventUserIdleSystemSleep named: \"x\"\nNo kernel assertions.";
        assert_eq!(parse_audio_out_assertions(out), Vec::new());
    }
}
