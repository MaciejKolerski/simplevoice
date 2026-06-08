//! Cross-platform system media playback control for the "Pause System Audio" feature.
//!
//! # Strategy per platform
//! - **macOS**  — AppleScript via `osascript` to detect and pause/resume individual apps
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

#[cfg(target_os = "macos")]
fn platform_pause() -> Vec<String> {
    // Check if audio is currently playing by checking macOS power assertions
    let output = std::process::Command::new("pmset")
        .arg("-g")
        .arg("assertions")
        .output()
        .ok();

    let is_playing = if let Some(out) = output {
        let stdout = String::from_utf8_lossy(&out.stdout);
        // "coreaudiod" creates an assertion when audio is actively playing
        stdout.contains("coreaudiod")
    } else {
        false
    };

    if is_playing {
        let _ = media_remote::send_command(media_remote::Command::Pause);
        vec!["__macos_media__".to_string()]
    } else {
        Vec::new()
    }
}

#[cfg(target_os = "macos")]
fn platform_resume(paused_apps: &[String]) {
    if paused_apps.iter().any(|p| p == "__macos_media__") {
        let _ = media_remote::send_command(media_remote::Command::Play);
    }
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
