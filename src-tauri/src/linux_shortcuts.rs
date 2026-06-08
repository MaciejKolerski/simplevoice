use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// Detects the currently active desktop environment on Linux.
pub fn detect_desktop_environment() -> String {
    let vars = ["XDG_CURRENT_DESKTOP", "GDMSESSION", "DESKTOP_SESSION"];
    for var in &vars {
        if let Ok(val) = std::env::var(var) {
            let val_lower = val.to_lowercase();
            if val_lower.contains("gnome") || val_lower.contains("unity") {
                return "gnome".to_string();
            } else if val_lower.contains("kde") {
                return "kde".to_string();
            } else if val_lower.contains("xfce") {
                return "xfce".to_string();
            } else if val_lower.contains("cinnamon") {
                return "cinnamon".to_string();
            } else if val_lower.contains("mate") {
                return "mate".to_string();
            } else if val_lower.contains("niri") {
                return "niri".to_string();
            } else if val_lower.contains("sway") {
                return "sway".to_string();
            } else if val_lower.contains("i3") {
                return "i3".to_string();
            } else if val_lower.contains("hyprland") {
                return "hyprland".to_string();
            }
        }
    }
    "unknown".to_string()
}

/// Translates a Tauri-formatted shortcut string (e.g. "Control+Shift+Space")
/// to the format expected by GNOME/GTK/XFCE (e.g. "<Primary><Shift>space").
fn translate_to_gnome_shortcut(shortcut: &str) -> String {
    let parts: Vec<&str> = shortcut.split('+').collect();
    let mut translated = Vec::new();
    let mut key = "";

    for part in parts {
        match part {
            "Control" | "CommandOrControl" => translated.push("<Primary>"),
            "Shift" => translated.push("<Shift>"),
            "Alt" => translated.push("<Alt>"),
            "Super" | "Command" => translated.push("<Super>"),
            k => key = k,
        }
    }

    let mut binding = translated.join("");
    let key_lower = key.to_lowercase();
    let normalized_key = match key_lower.as_str() {
        "space" => "space",
        other => {
            if other.len() == 1 {
                other
            } else {
                key // Keep original for keys like F1, Up, Down, PageUp, etc.
            }
        }
    };
    binding.push_str(normalized_key);
    binding
}

/// Translates a Tauri-formatted shortcut string to KDE format (e.g. "Ctrl+Shift+Space").
fn translate_to_kde_shortcut(shortcut: &str) -> String {
    let parts: Vec<&str> = shortcut.split('+').collect();
    let mut translated = Vec::new();
    let mut key = "";

    for part in parts {
        match part {
            "Control" | "CommandOrControl" => translated.push("Ctrl"),
            "Shift" => translated.push("Shift"),
            "Alt" => translated.push("Alt"),
            "Super" | "Command" => translated.push("Meta"),
            k => key = k,
        }
    }

    let mut binding = String::new();
    for (i, t) in translated.iter().enumerate() {
        if i > 0 {
            binding.push('+');
        }
        binding.push_str(t);
    }
    if !binding.is_empty() {
        binding.push('+');
    }
    binding.push_str(key);
    binding
}

/// Translates a Tauri-formatted shortcut to window manager specific syntax
fn translate_to_wm_shortcut(shortcut: &str, de: &str) -> String {
    let parts: Vec<&str> = shortcut.split('+').collect();
    let mut mods = Vec::new();
    let mut key = "";

    for part in parts {
        match part {
            "Control" | "CommandOrControl" => {
                mods.push(match de {
                    "hyprland" => "CTRL",
                    _ => "Ctrl",
                });
            }
            "Shift" => {
                mods.push(match de {
                    "hyprland" => "SHIFT",
                    _ => "Shift",
                });
            }
            "Alt" => {
                mods.push(match de {
                    "hyprland" => "ALT",
                    "i3" | "sway" => "Mod1",
                    _ => "Alt",
                });
            }
            "Super" | "Command" => {
                mods.push(match de {
                    "hyprland" => "SUPER",
                    "i3" | "sway" => "Mod4",
                    _ => "Mod", // Niri uses Mod by default
                });
            }
            k => key = k,
        }
    }

    let key_lower = key.to_lowercase();
    let normalized_key = match key_lower.as_str() {
        "space" => {
            if de == "hyprland" || de == "i3" || de == "sway" {
                "space"
            } else {
                "Space"
            }
        }
        other => {
            if other.len() == 1 {
                other
            } else {
                key
            }
        }
    };

    match de {
        "hyprland" => {
            let mods_str = mods.join("_");
            if mods_str.is_empty() {
                normalized_key.to_string()
            } else {
                format!("{}, {}", mods_str, normalized_key)
            }
        }
        _ => {
            // Niri, Sway, i3 join with +
            let mut binding = mods.join("+");
            if !binding.is_empty() {
                binding.push('+');
            }
            binding.push_str(normalized_key);
            binding
        }
    }
}

/// Helper to get target config file path for window managers
fn get_wm_config_path(de: &str) -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    match de {
        "niri" => {
            let dms_path = PathBuf::from(format!("{}/.config/niri/dms/binds.kdl", home));
            if dms_path.exists() {
                Some(dms_path)
            } else {
                Some(PathBuf::from(format!("{}/.config/niri/config.kdl", home)))
            }
        }
        "hyprland" => Some(PathBuf::from(format!("{}/.config/hypr/hyprland.conf", home))),
        "sway" => Some(PathBuf::from(format!("{}/.config/sway/config", home))),
        "i3" => {
            let p1 = PathBuf::from(format!("{}/.config/i3/config", home));
            if p1.exists() {
                Some(p1)
            } else {
                Some(PathBuf::from(format!("{}/.i3/config", home)))
            }
        }
        _ => None,
    }
}

/// Helper to parse a shell-like command line string and format it as individual
/// quoted arguments suitable for Niri's `spawn` directive.
/// e.g. `"/path/to/exe" --toggle` -> `"/path/to/exe" "--toggle"`
fn format_command_for_niri(cmd: &str) -> String {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let chars: Vec<char> = cmd.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '"' {
            in_quotes = !in_quotes;
        } else if c.is_whitespace() && !in_quotes {
            if !current.is_empty() {
                args.push(current.clone());
                current.clear();
            }
        } else {
            current.push(c);
        }
        i += 1;
    }
    if !current.is_empty() {
        args.push(current);
    }
    
    let quoted: Vec<String> = args.iter().map(|a| format!("\"{}\"", a)).collect();
    quoted.join(" ")
}

/// Helper to find the matching closing brace of the `binds` block and insert the
/// shortcut entries inside it.
fn insert_inside_binds_block(content: &str, section_content: &str) -> Option<String> {
    if let Some(start_idx) = content.find("binds {") {
        if let Some(brace_idx) = content[start_idx..].find('{') {
            let abs_brace_idx = start_idx + brace_idx;
            let mut brace_count = 1;
            let chars: Vec<(usize, char)> = content[abs_brace_idx + 1..].char_indices().collect();
            for (offset, c) in chars {
                if c == '{' {
                    brace_count += 1;
                } else if c == '}' {
                    brace_count -= 1;
                    if brace_count == 0 {
                        let closing_brace_pos = abs_brace_idx + 1 + offset;
                        let mut new_content = String::new();
                        new_content.push_str(&content[..closing_brace_pos]);
                        new_content.push_str("\n    ");
                        new_content.push_str(section_content.trim());
                        new_content.push('\n');
                        new_content.push_str(&content[closing_brace_pos..]);
                        return Some(new_content);
                    }
                }
            }
        }
    }
    None
}

/// Safely updates or appends a custom shortcut section in the window manager config file
fn update_wm_config_file(de: &str, command_to_run: &str, shortcut_str: &str, action_id: &str) -> Result<(), String> {
    let config_path = match get_wm_config_path(de) {
        Some(path) => path,
        None => return Ok(()),
    };

    // If config file doesn't exist, we don't create it (user doesn't use/configure this WM)
    if !config_path.exists() {
        return Ok(());
    }

    let wm_shortcut = translate_to_wm_shortcut(shortcut_str, de);

    let bind_entry = match de {
        "niri" => format!(
            "    \"{}\" {{ spawn {}; }}\n",
            wm_shortcut, format_command_for_niri(command_to_run)
        ),
        "hyprland" => format!(
            "bind = {}, exec, {}\n",
            wm_shortcut, command_to_run
        ),
        "sway" | "i3" => format!(
            "bindsym {} exec {}\n",
            wm_shortcut, command_to_run
        ),
        _ => return Ok(()),
    };

    let content = fs::read_to_string(&config_path)
        .map_err(|e| format!("Failed to read WM config file: {}", e))?;

    // Niri uses KDL, which uses C-style comments (//) instead of shell comments (#)
    let comment_prefix = if de == "niri" { "//" } else { "#" };

    let start_marker = format!("{} --- SIMPLEVOICE SHORTCUTS START [{}] ---", comment_prefix, action_id);
    let end_marker = format!("{} --- SIMPLEVOICE SHORTCUTS END [{}] ---", comment_prefix, action_id);

    let start_search = format!("SIMPLEVOICE SHORTCUTS START [{}]", action_id);
    let end_search = format!("SIMPLEVOICE SHORTCUTS END [{}]", action_id);

    // Unregister with nothing of ours in the file: leave it completely untouched
    // (avoids gratuitously rewriting the user's compositor config on startup).
    if shortcut_str.trim().is_empty() && !content.contains(&start_search) {
        return Ok(());
    }

    let mut new_content = String::new();
    let mut inside_section = false;
    let mut section_replaced = false;

    let mut section_content = String::new();
    if !shortcut_str.trim().is_empty() {
        section_content.push_str(&start_marker);
        section_content.push('\n');
        section_content.push_str(&bind_entry);
        section_content.push_str(&end_marker);
        section_content.push('\n');
    }

    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if line.contains(&start_search) {
            inside_section = true;
            new_content.push_str(&section_content);
            section_replaced = true;
            i += 1;
            continue;
        }
        if line.contains(&end_search) {
            inside_section = false;
            i += 1;
            continue;
        }
        if !inside_section {
            new_content.push_str(line);
            new_content.push('\n');
        }
        i += 1;
    }

    if !section_replaced && !section_content.is_empty() {
        if de == "niri" {
            if let Some(inserted_content) = insert_inside_binds_block(&new_content, &section_content) {
                new_content = inserted_content;
            } else {
                // Fallback: if no binds block found, wrap in binds { ... } and append to end
                let mut fallback_content = String::new();
                fallback_content.push_str(&start_marker);
                fallback_content.push('\n');
                fallback_content.push_str("binds {\n");
                fallback_content.push_str(&bind_entry);
                fallback_content.push_str("}\n");
                fallback_content.push_str(&end_marker);
                fallback_content.push('\n');

                if !new_content.ends_with('\n') {
                    new_content.push('\n');
                }
                new_content.push_str(&fallback_content);
            }
        } else {
            if !new_content.ends_with('\n') {
                new_content.push('\n');
            }
            new_content.push_str(&section_content);
        }
    }

    fs::write(&config_path, new_content)
        .map_err(|e| format!("Failed to write WM config file: {}", e))?;

    // Reload Sway or i3 configs if needed
    if de == "sway" {
        let _ = Command::new("swaymsg").arg("reload").status();
    } else if de == "i3" {
        let _ = Command::new("i3-msg").arg("reload").status();
    }

    Ok(())
}

/// Automatically repairs any old invalid shell-style (#) comments in KDL files to correct C-style (//) comments on startup
pub fn repair_wm_configs() {
    let de = detect_desktop_environment();
    if de != "niri" {
        return;
    }

    if let Some(config_path) = get_wm_config_path("niri") {
        if config_path.exists() {
            if let Ok(content) = fs::read_to_string(&config_path) {
                if content.contains("# --- SIMPLEVOICE SHORTCUTS START") || content.contains("# --- SIMPLEVOICE SHORTCUTS END") {
                    let mut new_content = String::new();
                    for line in content.lines() {
                        if line.contains("SIMPLEVOICE SHORTCUTS") && line.trim().starts_with('#') {
                            // Replace '#' with '//'
                            let repaired = line.replacen('#', "//", 1);
                            new_content.push_str(&repaired);
                        } else {
                            new_content.push_str(line);
                        }
                        new_content.push('\n');
                    }
                    let _ = fs::write(&config_path, new_content);
                    println!("Repaired KDL comment syntax in Niri config file.");
                }
            }
        }
    }
}

/// Parses a gsettings array string, e.g. "['path1', 'path2']" or "@as []", into a Vec of Strings.
fn parse_gsettings_array(s: &str) -> Vec<String> {
    if s.contains("@as []") || s == "[]" || s.trim().is_empty() {
        return Vec::new();
    }
    let s = s.trim().trim_start_matches('[').trim_end_matches(']');
    s.split(',')
        .map(|item| item.trim().trim_matches('\'').trim_matches('"').to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

/// Formats a list of paths back into a gsettings array string.
fn format_gsettings_array(paths: &[String]) -> String {
    if paths.is_empty() {
        return "@as []".to_string();
    }
    let quoted: Vec<String> = paths.iter().map(|p| format!("'{}'", p)).collect();
    format!("[{}]", quoted.join(", "))
}

/// Registers a native Linux keybinding in the desktop environment.
pub fn register_native_shortcut(
    name: &str,
    command_to_run: &str,
    shortcut_str: &str,
    action_id: &str,
) -> Result<(), String> {
    if shortcut_str.trim().is_empty() {
        return unregister_native_shortcut(action_id);
    }

    let de = detect_desktop_environment();
    match de.as_str() {
        "gnome" => register_gnome_shortcut(name, command_to_run, shortcut_str, action_id),
        "cinnamon" => register_cinnamon_shortcut(name, command_to_run, shortcut_str, action_id),
        "mate" => register_mate_shortcut(name, command_to_run, shortcut_str, action_id),
        "xfce" => register_xfce_shortcut(command_to_run, shortcut_str, action_id),
        "kde" => register_kde_shortcut(name, command_to_run, shortcut_str, action_id),
        "niri" | "hyprland" | "sway" | "i3" => {
            update_wm_config_file(&de, command_to_run, shortcut_str, action_id)
        }
        _ => Err(format!("Unsupported desktop environment: {}", de)),
    }
}

/// Unregisters a native Linux keybinding.
pub fn unregister_native_shortcut(action_id: &str) -> Result<(), String> {
    let de = detect_desktop_environment();
    match de.as_str() {
        "gnome" => unregister_gnome_shortcut(action_id),
        "cinnamon" => unregister_cinnamon_shortcut(action_id),
        "mate" => unregister_mate_shortcut(action_id),
        "xfce" => unregister_xfce_shortcut_by_action(action_id),
        "kde" => unregister_kde_shortcut(action_id),
        "niri" | "hyprland" | "sway" | "i3" => {
            update_wm_config_file(&de, "", "", action_id)
        }
        _ => Ok(()),
    }
}

fn register_gnome_shortcut(
    name: &str,
    command_to_run: &str,
    shortcut_str: &str,
    action_id: &str,
) -> Result<(), String> {
    let path = format!(
        "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/custom-simplevoice-{}/",
        action_id
    );
    let schema_key = "org.gnome.settings-daemon.plugins.media-keys.custom-keybinding";
    let binding = translate_to_gnome_shortcut(shortcut_str);

    let output = Command::new("gsettings")
        .args(&[
            "get",
            "org.gnome.settings-daemon.plugins.media-keys",
            "custom-keybindings",
        ])
        .output()
        .map_err(|e| format!("Failed to query gsettings custom-keybindings: {}", e))?;

    let current_list_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let mut paths = parse_gsettings_array(&current_list_str);

    if !paths.contains(&path) {
        paths.push(path.clone());
    }

    let formatted_list = format_gsettings_array(&paths);
    let status = Command::new("gsettings")
        .args(&[
            "set",
            "org.gnome.settings-daemon.plugins.media-keys",
            "custom-keybindings",
            &formatted_list,
        ])
        .status()
        .map_err(|e| format!("Failed to update custom-keybindings list: {}", e))?;

    if !status.success() {
        return Err("gsettings set custom-keybindings returned non-zero".to_string());
    }

    let schema_path = format!("{}:{}", schema_key, path);

    let _ = Command::new("gsettings")
        .args(&["set", &schema_path, "name", name])
        .status();

    let _ = Command::new("gsettings")
        .args(&["set", &schema_path, "command", command_to_run])
        .status();

    let _ = Command::new("gsettings")
        .args(&["set", &schema_path, "binding", &binding])
        .status();

    Ok(())
}

fn unregister_gnome_shortcut(action_id: &str) -> Result<(), String> {
    let path = format!(
        "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/custom-simplevoice-{}/",
        action_id
    );

    let output = Command::new("gsettings")
        .args(&[
            "get",
            "org.gnome.settings-daemon.plugins.media-keys",
            "custom-keybindings",
        ])
        .output()
        .map_err(|e| format!("Failed to query gsettings custom-keybindings: {}", e))?;

    let current_list_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let mut paths = parse_gsettings_array(&current_list_str);

    if paths.contains(&path) {
        paths.retain(|p| p != &path);
        let formatted_list = format_gsettings_array(&paths);
        let _ = Command::new("gsettings")
            .args(&[
                "set",
                "org.gnome.settings-daemon.plugins.media-keys",
                "custom-keybindings",
                &formatted_list,
            ])
            .status();

        let schema_key = "org.gnome.settings-daemon.plugins.media-keys.custom-keybinding";
        let schema_path = format!("{}:{}", schema_key, path);
        let _ = Command::new("gsettings")
            .args(&["reset-recursively", &schema_path])
            .status();
    }

    Ok(())
}

fn register_cinnamon_shortcut(
    name: &str,
    command_to_run: &str,
    shortcut_str: &str,
    action_id: &str,
) -> Result<(), String> {
    let path = format!(
        "/org/cinnamon/desktop/keybindings/custom-keybindings/custom-simplevoice-{}/",
        action_id
    );
    let schema_key = "org.cinnamon.desktop.keybindings.custom-keybinding";
    let binding = translate_to_gnome_shortcut(shortcut_str);

    let output = Command::new("gsettings")
        .args(&[
            "get",
            "org.cinnamon.desktop.keybindings",
            "custom-keybindings",
        ])
        .output()
        .map_err(|e| format!("Failed to query cinnamon keybindings: {}", e))?;

    let current_list_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let mut paths = parse_gsettings_array(&current_list_str);

    if !paths.contains(&path) {
        paths.push(path.clone());
    }

    let formatted_list = format_gsettings_array(&paths);
    let _ = Command::new("gsettings")
        .args(&[
            "set",
            "org.cinnamon.desktop.keybindings",
            "custom-keybindings",
            &formatted_list,
        ])
        .status();

    let schema_path = format!("{}:{}", schema_key, path);
    let _ = Command::new("gsettings").args(&["set", &schema_path, "name", name]).status();
    let _ = Command::new("gsettings").args(&["set", &schema_path, "command", command_to_run]).status();
    let _ = Command::new("gsettings").args(&["set", &schema_path, "binding", &binding]).status();

    Ok(())
}

fn unregister_cinnamon_shortcut(action_id: &str) -> Result<(), String> {
    let path = format!(
        "/org/cinnamon/desktop/keybindings/custom-keybindings/custom-simplevoice-{}/",
        action_id
    );

    let output = Command::new("gsettings")
        .args(&[
            "get",
            "org.cinnamon.desktop.keybindings",
            "custom-keybindings",
        ])
        .output()
        .map_err(|e| format!("Failed to query cinnamon keybindings: {}", e))?;

    let current_list_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let mut paths = parse_gsettings_array(&current_list_str);

    if paths.contains(&path) {
        paths.retain(|p| p != &path);
        let formatted_list = format_gsettings_array(&paths);
        let _ = Command::new("gsettings")
            .args(&[
                "set",
                "org.cinnamon.desktop.keybindings",
                "custom-keybindings",
                &formatted_list,
            ])
            .status();
    }

    Ok(())
}

fn register_mate_shortcut(
    name: &str,
    command_to_run: &str,
    shortcut_str: &str,
    action_id: &str,
) -> Result<(), String> {
    let path = format!(
        "/org/mate/desktop/keybindings/custom-keybindings/custom-simplevoice-{}/",
        action_id
    );
    let schema_key = "org.mate.SettingsDaemon.plugins.media-keys.custom-keybinding";
    let binding = translate_to_gnome_shortcut(shortcut_str);

    let output = Command::new("gsettings")
        .args(&[
            "get",
            "org.mate.SettingsDaemon.plugins.media-keys",
            "custom-keybindings",
        ])
        .output()
        .map_err(|e| format!("Failed to query mate keybindings: {}", e))?;

    let current_list_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let mut paths = parse_gsettings_array(&current_list_str);

    if !paths.contains(&path) {
        paths.push(path.clone());
    }

    let formatted_list = format_gsettings_array(&paths);
    let _ = Command::new("gsettings")
        .args(&[
            "set",
            "org.mate.SettingsDaemon.plugins.media-keys",
            "custom-keybindings",
            &formatted_list,
        ])
        .status();

    let schema_path = format!("{}:{}", schema_key, path);
    let _ = Command::new("gsettings").args(&["set", &schema_path, "name", name]).status();
    let _ = Command::new("gsettings").args(&["set", &schema_path, "command", command_to_run]).status();
    let _ = Command::new("gsettings").args(&["set", &schema_path, "binding", &binding]).status();

    Ok(())
}

fn unregister_mate_shortcut(action_id: &str) -> Result<(), String> {
    let path = format!(
        "/org/mate/desktop/keybindings/custom-keybindings/custom-simplevoice-{}/",
        action_id
    );

    let output = Command::new("gsettings")
        .args(&[
            "get",
            "org.mate.SettingsDaemon.plugins.media-keys",
            "custom-keybindings",
        ])
        .output()
        .map_err(|e| format!("Failed to query mate keybindings: {}", e))?;

    let current_list_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let mut paths = parse_gsettings_array(&current_list_str);

    if paths.contains(&path) {
        paths.retain(|p| p != &path);
        let formatted_list = format_gsettings_array(&paths);
        let _ = Command::new("gsettings")
            .args(&[
                "set",
                "org.mate.SettingsDaemon.plugins.media-keys",
                "custom-keybindings",
                &formatted_list,
            ])
            .status();
    }

    Ok(())
}

fn register_xfce_shortcut(
    command_to_run: &str,
    shortcut_str: &str,
    action_id: &str,
) -> Result<(), String> {
    let suffix = format!("--{}", action_id);
    unregister_xfce_shortcuts_for_command(&suffix);

    let binding = translate_to_gnome_shortcut(shortcut_str);
    let property_path = format!("/commands/custom/{}", binding);

    let status = Command::new("xfconf-query")
        .args(&[
            "-c",
            "xfce4-keyboard-shortcuts",
            "-p",
            &property_path,
            "-n",
            "-t",
            "string",
            "-s",
            command_to_run,
        ])
        .status()
        .map_err(|e| format!("Failed to run xfconf-query: {}", e))?;

    if !status.success() {
        return Err("xfconf-query set shortcut returned non-zero".to_string());
    }

    Ok(())
}

fn unregister_xfce_shortcut_by_action(action_id: &str) -> Result<(), String> {
    let suffix = format!("--{}", action_id);
    unregister_xfce_shortcuts_for_command(&suffix);
    Ok(())
}

fn unregister_xfce_shortcuts_for_command(command_substring: &str) {
    let output = Command::new("xfconf-query")
        .args(&["-c", "xfce4-keyboard-shortcuts", "-l", "-v"])
        .output();

    if let Ok(out) = output {
        let stdout = String::from_utf8_lossy(&out.stdout);
        for line in stdout.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if !parts.is_empty() {
                let property = parts[0];
                let value = line[property.len()..].trim();
                if value.contains(command_substring) {
                    let _ = Command::new("xfconf-query")
                        .args(&["-c", "xfce4-keyboard-shortcuts", "-p", property, "-r"])
                        .status();
                }
            }
        }
    }
}

fn register_kde_shortcut(
    name: &str,
    command_to_run: &str,
    shortcut_str: &str,
    action_id: &str,
) -> Result<(), String> {
    let home = std::env::var("HOME").map_err(|_| "HOME environment variable not set".to_string())?;
    let desktop_dir = format!("{}/.local/share/applications", home);
    std::fs::create_dir_all(&desktop_dir)
        .map_err(|e| format!("Failed to create applications directory: {}", e))?;

    let desktop_filename = format!("simplevoice-{}.desktop", action_id);
    let desktop_path = format!("{}/{}", desktop_dir, desktop_filename);

    let desktop_content = format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name={}\n\
         Exec={}\n\
         Icon=simplevoice\n\
         NoDisplay=true\n\
         Terminal=false\n",
        name, command_to_run
    );

    std::fs::write(&desktop_path, desktop_content)
        .map_err(|e| format!("Failed to write KDE .desktop file: {}", e))?;

    let kwriteconfig_cmd = if Command::new("kwriteconfig6").arg("--version").status().is_ok() {
        "kwriteconfig6"
    } else if Command::new("kwriteconfig5").arg("--version").status().is_ok() {
        "kwriteconfig5"
    } else {
        "kwriteconfig"
    };

    let binding = translate_to_kde_shortcut(shortcut_str);
    let group = desktop_filename;

    let _ = Command::new(kwriteconfig_cmd)
        .args(&[
            "--file",
            "kglobalshortcutsrc",
            "--group",
            &group,
            "--key",
            "_k_friendly_name",
            name,
        ])
        .status();

    let _ = Command::new(kwriteconfig_cmd)
        .args(&[
            "--file",
            "kglobalshortcutsrc",
            "--group",
            &group,
            "--key",
            "_launch",
            &format!("{},none,{}", binding, name),
        ])
        .status();

    let _ = Command::new("qdbus")
        .args(&[
            "org.kde.kglobalaccel",
            "/kglobalaccel",
            "org.kde.KGlobalAccel.reconfigure",
        ])
        .status();

    let _ = Command::new("qdbus")
        .args(&["org.kde.KWin", "/KWin", "org.kde.KWin.reconfigure"])
        .status();

    Ok(())
}

fn unregister_kde_shortcut(action_id: &str) -> Result<(), String> {
    let home = std::env::var("HOME").map_err(|_| "HOME environment variable not set".to_string())?;
    let desktop_filename = format!("simplevoice-{}.desktop", action_id);
    let desktop_path = format!("{}/.local/share/applications/{}", home, desktop_filename);

    let _ = std::fs::remove_file(&desktop_path);

    let kwriteconfig_cmd = if Command::new("kwriteconfig6").arg("--version").status().is_ok() {
        "kwriteconfig6"
    } else if Command::new("kwriteconfig5").arg("--version").status().is_ok() {
        "kwriteconfig5"
    } else {
        "kwriteconfig"
    };

    let _ = Command::new(kwriteconfig_cmd)
        .args(&[
            "--file",
            "kglobalshortcutsrc",
            "--group",
            &desktop_filename,
            "--key",
            "_launch",
            "@null",
        ])
        .status();

    Ok(())
}
