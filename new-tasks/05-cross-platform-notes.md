# Cross-Platform Implementation Notes

## 1. Keyboard Simulation (Enigo)
- **macOS:** Requires "Accessibility" permissions. Users must enable it in System Settings.
- **Windows:** Generally works out of the box for standard applications (Administrator privileges may be required for some system apps).
- **Linux:** Requires X11. The current `enigo` setup uses `x11rb`, which is compatible with most X11 desktop environments.

## 2. Autostart (tauri-plugin-autostart)
- **macOS:** Adds an entry to ~/Library/LaunchAgents.
- **Windows:** Adds a registry entry in HKCU\Software\Microsoft\Windows\CurrentVersion\Run.
- **Linux:** Creates a .desktop file in ~/.config/autostart/.

## 3. Permissions Strategy
- We will add checks to inform the user if permissions (like Accessibility on macOS) are missing.
