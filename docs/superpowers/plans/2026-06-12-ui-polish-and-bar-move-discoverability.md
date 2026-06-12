# UI Polish and Recording-Bar Move Discoverability Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make moving the recording bar discoverable from Settings on every platform (hint + unlock switch + reset button + amber ⌘ feedback on macOS), and run a consistency/accessibility/responsiveness polish pass over the existing UI.

**Architecture:** Backend work is confined to `src-tauri/src/lib.rs` (one new command, one extracted helper, one event emit added to the existing ⌘-polling thread). Frontend work adds one shared `SettingRow` component, new rows in SettingsView, and targeted edits in ModelsView / TranscriptionsView / UsageView / App.css. config.json stays the source of truth for backend-read settings (never mirror localStorage into it on mount).

**Tech Stack:** Tauri 2 (Rust), React 19, TypeScript strict, Tailwind v4, i18next (en/pl/de), sonner toasts, Lucide icons. Package manager is **pnpm only**.

**Spec:** `docs/superpowers/specs/2026-06-12-ui-polish-and-bar-move-discoverability-design.md`

**Conventions that bind every task:**
- Comments: default to none; if needed explain *why* in 1-2 lines, in English. No emojis anywhere.
- Verify with `pnpm lint` (frontend) and `cargo check` inside `src-tauri/` (backend) before each commit.
- Commit after every task, conventional-commit style, as the user, **no Co-Authored-By trailer**.
- There is no frontend test runner in this repo; "tests" for UI tasks are `pnpm lint` + `pnpm check:i18n` + the manual checklist in Task 11.

---

### Task 1: Extract the default-placement helper (Rust)

The first-show placement math is duplicated in the macOS and Linux/Windows variants of `update_recording_window_visibility`. The reset command (Task 2) needs it too, so extract it once.

**Files:**
- Modify: `src-tauri/src/lib.rs` (macOS block ~lines 506-532, Linux/Windows block ~lines 649-675)

- [ ] **Step 1: Add the helper above `update_recording_window_visibility` (above line 485)**

```rust
/// Top-center default placement for the overlay: used on its very first show
/// and by the reset-position command. macOS sits 36 logical px below the top
/// edge; Linux/Windows use 5% of the monitor height.
fn apply_default_recording_window_position(window: &tauri::WebviewWindow) {
    if let Some(monitor) = window.current_monitor().ok().flatten() {
        let size = monitor.size();
        let pos = monitor.position();
        let scale_factor = monitor.scale_factor();

        let win_w = 200.0;
        let x = pos.x + ((size.width as f64 - win_w * scale_factor) / 2.0) as i32;

        #[cfg(target_os = "macos")]
        let y = pos.y + (36.0 * scale_factor) as i32;
        #[cfg(not(target_os = "macos"))]
        let y = pos.y + (size.height as f64 * 0.05) as i32;

        let _ = window.set_position(tauri::Position::Physical(
            tauri::PhysicalPosition::new(x, y),
        ));
    }
}
```

- [ ] **Step 2: Replace both duplicated `if !positioned { ... }` blocks**

In the macOS `update_recording_window_visibility` (the block currently at lines 515-530) and in the Linux/Windows variant (currently lines 658-673), replace the whole `if !positioned { if let Some(monitor) = ... }` block with:

```rust
                if !positioned {
                    apply_default_recording_window_position(&window);
                }
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check` (cwd: `src-tauri/`)
Expected: exit 0, no warnings about unused variables (the old `size`/`pos`/`scale_factor` bindings are gone with the blocks).

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "refactor(overlay): extract default recording-window placement helper"
```

---

### Task 2: `reset_recording_window_position` command (Rust)

**Files:**
- Modify: `src-tauri/src/lib.rs` (new command near `set_recording_window_locked` at line 2082; registration in `generate_handler!` at lines 2942-2998)

- [ ] **Step 1: Add the command directly below `set_recording_window_locked` (after line 2115)**

```rust
#[tauri::command]
fn reset_recording_window_position(app_handle: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app_handle.get_webview_window("recording_window") {
        apply_default_recording_window_position(&window);
    }

    // Clear the custom-position flag so the next first-show recomputes the
    // default. The Moved event fired by set_position above may re-save the
    // default coordinates as a "custom" position (exactly like the existing
    // first-show flow does); either ordering leaves the bar at the default
    // spot, so the race is benign.
    let app_local_data = app_handle
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&app_local_data).map_err(|e| e.to_string())?;
    let config_path = app_local_data.join("config.json");
    let _guard = CONFIG_FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let mut json = if config_path.exists() {
        let content = std::fs::read_to_string(&config_path).map_err(|e| e.to_string())?;
        serde_json::from_str::<serde_json::Value>(&content)
            .unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    if let Some(obj) = json.as_object_mut() {
        obj.insert(
            "recording_window_has_custom_pos".to_string(),
            serde_json::json!(false),
        );
    }

    let serialized = serde_json::to_string_pretty(&json).map_err(|e| e.to_string())?;
    std::fs::write(&config_path, serialized).map_err(|e| e.to_string())?;

    Ok(())
}
```

- [ ] **Step 2: Register it in `generate_handler!`**

In the `invoke_handler(tauri::generate_handler![...])` list, add one line after `set_recording_window_locked,` (line 2992):

```rust
            reset_recording_window_position,
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check` (cwd: `src-tauri/`)
Expected: exit 0.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat(overlay): command to reset the recording bar to its default position"
```

---

### Task 3: macOS ⌘ feedback — emit lock-status from the polling thread (Rust)

The overlay already listens for `recording-window-lock-status` (`src/views/RecordingWindowView.tsx:133`) and renders an amber glow when unlocked. On macOS nothing emits it today.

**Files:**
- Modify: `src-tauri/src/lib.rs` (the ⌘-polling thread, lines 2824-2873)

- [ ] **Step 1: Emit the event when the ⌘ state flips**

Inside `if command_pressed != last_command_state {` (line 2848), right after `last_command_state = command_pressed;` (line 2849), add:

```rust
                                        // Drive the overlay's amber "movable" glow while Cmd is
                                        // held. Visual signal only: this does not persist
                                        // recording_window_locked on macOS.
                                        let _ = app_handle
                                            .emit("recording-window-lock-status", !command_pressed);
```

`tauri::Emitter` is already imported at line 22; `app_handle` is the thread's clone and is in scope.

- [ ] **Step 2: Verify it compiles**

Run: `cargo check` (cwd: `src-tauri/`)
Expected: exit 0.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat(macos): amber overlay glow while Cmd-drag is available"
```

---

### Task 4: i18n keys for the new settings rows and the usage empty state

**Files:**
- Modify: `src/i18n/locales/en.json`, `src/i18n/locales/pl.json`, `src/i18n/locales/de.json`

- [ ] **Step 1: Add to the `settings` object in `en.json`** (keep alphabetical-by-feature grouping loose — append near the `recording*` keys):

```json
"barPositionMoveTitle": "Move the recording bar",
"barPositionMoveDescMac": "Hold <kbd>⌘ Cmd</kbd> and drag the bar anywhere on screen. Its position is saved automatically.",
"barPositionUnlockTitle": "Unlock bar position",
"barPositionUnlockDesc": "When unlocked, drag the bar to move it — the amber glow means it can be moved. Lock it again to let clicks pass through.",
"barPositionResetTitle": "Reset bar position",
"barPositionResetDesc": "Move the bar back to its default spot.",
"barPositionReset": "Reset",
"barPositionResetDone": "Bar position has been reset",
"barPositionResetError": "Could not reset the bar position"
```

And to the `usage` object:

```json
"emptyTitle": "No activity yet",
"emptyHint": "Transcriptions you make will show up here."
```

- [ ] **Step 2: Add the same keys to `pl.json`**

`settings`:

```json
"barPositionMoveTitle": "Przesuwanie paska nagrywania",
"barPositionMoveDescMac": "Przytrzymaj <kbd>⌘ Cmd</kbd> i przeciągnij pasek w dowolne miejsce ekranu. Pozycja zapisuje się automatycznie.",
"barPositionUnlockTitle": "Odblokuj pozycję paska",
"barPositionUnlockDesc": "Po odblokowaniu przeciągnij pasek, aby go przesunąć — bursztynowa poświata oznacza, że można go przesuwać. Zablokuj ponownie, aby kliknięcia przechodziły przez pasek.",
"barPositionResetTitle": "Resetuj pozycję paska",
"barPositionResetDesc": "Przywróć pasek do domyślnego położenia.",
"barPositionReset": "Resetuj",
"barPositionResetDone": "Pozycja paska została zresetowana",
"barPositionResetError": "Nie udało się zresetować pozycji paska"
```

`usage`:

```json
"emptyTitle": "Brak aktywności",
"emptyHint": "Twoje transkrypcje pojawią się tutaj."
```

- [ ] **Step 3: Add the same keys to `de.json`**

`settings`:

```json
"barPositionMoveTitle": "Aufnahmeleiste verschieben",
"barPositionMoveDescMac": "Halte <kbd>⌘ Cmd</kbd> gedrückt und ziehe die Leiste an eine beliebige Stelle. Die Position wird automatisch gespeichert.",
"barPositionUnlockTitle": "Leistenposition entsperren",
"barPositionUnlockDesc": "Nach dem Entsperren lässt sich die Leiste per Ziehen verschieben — das bernsteinfarbene Leuchten zeigt an, dass sie beweglich ist. Sperre sie wieder, damit Klicks hindurchgehen.",
"barPositionResetTitle": "Leistenposition zurücksetzen",
"barPositionResetDesc": "Setzt die Leiste auf ihre Standardposition zurück.",
"barPositionReset": "Zurücksetzen",
"barPositionResetDone": "Leistenposition wurde zurückgesetzt",
"barPositionResetError": "Leistenposition konnte nicht zurückgesetzt werden"
```

`usage`:

```json
"emptyTitle": "Noch keine Aktivität",
"emptyHint": "Deine Transkriptionen erscheinen hier."
```

- [ ] **Step 4: Verify key parity**

Run: `pnpm check:i18n`
Expected: exit 0, no missing-key report.

- [ ] **Step 5: Commit**

```bash
git add src/i18n/locales/en.json src/i18n/locales/pl.json src/i18n/locales/de.json
git commit -m "feat(i18n): strings for bar-position settings and usage empty state"
```

---

### Task 5: Shared `SettingRow` component + SettingsView refactor

**Files:**
- Create: `src/components/ui/setting-row.tsx`
- Modify: `src/views/SettingsView.tsx`

- [ ] **Step 1: Create `src/components/ui/setting-row.tsx`**

```tsx
import { ReactNode } from "react";
import { Label } from "@/components/ui/label";

type SettingRowProps = {
  title: ReactNode;
  description?: ReactNode;
  /** "row" puts the control to the right of the text (default); "column" stacks it below. */
  layout?: "row" | "column";
  children?: ReactNode;
  className?: string;
  "data-tour"?: string;
};

/**
 * One bordered row inside a settings card. Encapsulates the
 * title/description/control pattern shared by SettingsView and ModelsView so
 * spacing, typography and dividers stay consistent.
 */
export function SettingRow({
  title,
  description,
  layout = "row",
  children,
  className = "",
  ...rest
}: SettingRowProps) {
  if (layout === "column") {
    return (
      <div
        className={`flex flex-col p-5 border-b border-border last:border-b-0 ${className}`}
        {...rest}
      >
        <Label className={description ? "mb-1" : "mb-3"}>{title}</Label>
        {description && <p className="text-muted text-[13px] mb-3">{description}</p>}
        {children}
      </div>
    );
  }

  return (
    <div
      className={`flex justify-between items-center gap-6 p-5 border-b border-border last:border-b-0 ${className}`}
      {...rest}
    >
      <div className="flex-1 min-w-0">
        <div className="text-fg font-medium mb-1">{title}</div>
        {description && (
          <div className="text-muted text-[13px] leading-snug">{description}</div>
        )}
      </div>
      {children}
    </div>
  );
}
```

- [ ] **Step 2: Refactor SettingsView rows to `SettingRow`**

Add the import: `import { SettingRow } from "@/components/ui/setting-row";`

Convert every settings row. Mapping (current line → props; the control element stays exactly as it is and becomes `children`):

| Current line | layout | title | description | notes |
|---|---|---|---|---|
| 614 interface language | column | `t("settings.interfaceLanguage")` | — | |
| 644 microphone | column | `t("settings.inputMicrophone")` | — | |
| 665 transcription language | column | `t("settings.transcriptionLanguage")` | — | keep `data-tour="language-select"`; keep the trailing help `<p>` as part of `children` after the Select but change its class to `text-muted text-[13px] mt-2` (size unification) |
| 695 GPU | row | `t("settings.gpuAcceleration")` | `t("settings.gpuAccelerationDesc")` | drops its `text-xs` for the standard 13px |
| 714 live toggle | row | `t("settings.liveTranscription")` | `t("settings.liveTranscriptionDesc")` | |
| 732 live autopaste | row | `t("settings.liveAutopaste")` | `t("settings.liveAutopasteDesc")` | stays inside the disabled wrapper div |
| 748 overlay text | column | `t("settings.liveOverlayText")` | `t("settings.liveOverlayTextDesc")` | |
| 775 live speed | column | `t("settings.liveSpeed")` | `t("settings.liveSpeedDesc")` | |
| 815 autostart | row | `t("settings.autoStart")` | `t("settings.autoStartDesc")` | drops `text-xs` |
| 828 VAD | row | `t("settings.vad")` | `t("settings.vadDesc")` | |
| 840 sound effects | row | `t("settings.soundEffects")` | `t("settings.soundEffectsDesc")` | |
| 850 pause audio | row | `t("settings.pauseSystemAudio")` | `t("settings.pauseSystemAudioDesc")` | |
| 864 overlay window mode | row | `t("settings.recordingOverlayWindow")` | `t("settings.recordingOverlayWindowDesc")` | gains the standard `border-b ... last:border-b-0` (new rows follow it in Task 6) |
| 899 record shortcut | row | `t("settings.startStopRecording")` | `t("settings.startStopRecordingDesc")` | |
| 918 copy shortcut | row | `t("settings.copyLastTranscription")` | `t("settings.copyLastTranscriptionDesc")` | |
| 1012 accessibility | row | JSX: label text + status dot span (move the whole `flex items-center gap-2` title content into `title={...}`) | existing desc JSX incl. warning span | |
| 1056 microphone permission | row | same pattern as accessibility | same | |
| 1114 about | row | `Simplevoice` (literal brand string stays untranslated) | existing version line | |

Example conversion (VAD row, line 828) — before:

```tsx
<div className="flex justify-between items-center gap-6 p-5 border-b border-border last:border-b-0">
  <div className="min-w-0">
    <div className="text-fg font-medium mb-1">{t("settings.vad")}</div>
    <div className="text-muted text-[13px]">{t("settings.vadDesc")}</div>
  </div>
  <Switch checked={vadEnabled} onCheckedChange={handleVadToggle} />
</div>
```

after:

```tsx
<SettingRow title={t("settings.vad")} description={t("settings.vadDesc")}>
  <Switch checked={vadEnabled} onCheckedChange={handleVadToggle} />
</SettingRow>
```

Example conversion (permissions row with status dot, line 1012) — the title prop takes the composed JSX:

```tsx
<SettingRow
  title={
    <span className="flex items-center gap-2">
      {t("settings.accessibility")}
      <span
        className={`inline-block w-2 h-2 rounded-full ${
          accessibilityGranted
            ? "bg-success shadow-[0_0_6px_rgba(52,211,153,0.5)]"
            : "bg-warning shadow-[0_0_6px_rgba(251,191,36,0.5)] animate-pulse"
        }`}
      />
    </span>
  }
  description={
    <>
      {t("settings.accessibilityDesc")}
      {!accessibilityGranted && (
        <span className="text-warning font-medium"> {t("settings.accessibilityNotGranted")}</span>
      )}
    </>
  }
>
  {/* existing Button / granted badge unchanged */}
</SettingRow>
```

Do NOT convert: the Linux/Wayland warning block (line 938), the shortcut-capture overlay (line 1138 onward), section headers.

- [ ] **Step 3: Typecheck**

Run: `pnpm lint`
Expected: exit 0.

- [ ] **Step 4: Commit**

```bash
git add src/components/ui/setting-row.tsx src/views/SettingsView.tsx
git commit -m "refactor(ui): shared SettingRow component for settings cards"
```

---

### Task 6: Bar-position rows in SettingsView

**Files:**
- Modify: `src/views/SettingsView.tsx`

- [ ] **Step 1: Add imports and state**

Add to the imports: `import { toast } from "sonner";` (pattern used in TranscriptionsView).

Add state + sync effect near the other state hooks (the event also fires from the tray toggle and, after Task 3, from the macOS ⌘ thread — harmless there since macOS renders no switch):

```tsx
const [barUnlocked, setBarUnlocked] = useState(false);

useEffect(() => {
  invoke<boolean>("is_recording_window_locked_cmd")
    .then((locked) => setBarUnlocked(!locked))
    .catch(() => {});
  const unlisten = listen<boolean>("recording-window-lock-status", (event) => {
    setBarUnlocked(!event.payload);
  });
  return () => {
    unlisten.then((f) => f());
  };
}, []);
```

- [ ] **Step 2: Add handlers near the other handlers**

```tsx
const handleBarLockToggle = async (checked: boolean) => {
  setBarUnlocked(checked);
  try {
    await invoke("set_recording_window_locked", { locked: !checked });
  } catch (err) {
    console.error("Failed to toggle recording bar lock:", err);
  }
};

const handleResetBarPosition = async () => {
  try {
    await invoke("reset_recording_window_position");
    toast.success(t("settings.barPositionResetDone"));
  } catch (err) {
    console.error("Failed to reset recording bar position:", err);
    toast.error(t("settings.barPositionResetError"));
  }
};
```

- [ ] **Step 3: Add the rows**

Inside the existing `{(isMac || platform === "linux" || platform === "windows") && (...)}` block of the Recording & Feedback section, directly after the overlay-window-mode `SettingRow`, add (wrap the block's children in a fragment if needed):

```tsx
{isMac && (
  <SettingRow
    title={t("settings.barPositionMoveTitle")}
    description={
      <Trans
        i18nKey="settings.barPositionMoveDescMac"
        components={{
          kbd: (
            <kbd className="inline-flex items-center justify-center px-1.5 py-0.5 mx-0.5 rounded-md border border-border bg-surface-active font-mono text-[11px] text-foreground" />
          ),
        }}
      />
    }
  />
)}
{(platform === "linux" || platform === "windows") && (
  <SettingRow
    title={t("settings.barPositionUnlockTitle")}
    description={t("settings.barPositionUnlockDesc")}
  >
    <Switch checked={barUnlocked} onCheckedChange={handleBarLockToggle} />
  </SettingRow>
)}
<SettingRow
  title={t("settings.barPositionResetTitle")}
  description={t("settings.barPositionResetDesc")}
>
  <Button variant="outline" size="sm" onClick={handleResetBarPosition}>
    {t("settings.barPositionReset")}
  </Button>
</SettingRow>
```

`Trans` is already imported at line 8. The `<kbd>` element in the translation string maps to the `components.kbd` slot.

- [ ] **Step 4: Typecheck and i18n check**

Run: `pnpm lint && pnpm check:i18n`
Expected: both exit 0.

- [ ] **Step 5: Commit**

```bash
git add src/views/SettingsView.tsx
git commit -m "feat(settings): discoverable recording-bar position controls"
```

---

### Task 7: ModelsView cloud-provider rows — SettingRow, responsive widths, label dedupe

**Files:**
- Modify: `src/views/ModelsView.tsx` (provider config rows at lines 1001-1167; `PROVIDER_LABELS` at 202-207)

- [ ] **Step 1: Convert the five provider rows to `SettingRow`**

Import: `import { SettingRow } from "@/components/ui/setting-row";`

Rows at lines 1002 (provider), 1026 (API key), 1062 (model), 1129 (custom model id), 1152 (base URL) become `SettingRow layout="row"` with `className="flex-wrap"` so the control wraps under the label at narrow widths. Titles/descriptions: `t("models.providerLabel")`/`t("models.providerDesc")` etc., exactly the strings already present.

- [ ] **Step 2: Make the fixed control widths flexible**

In those rows replace every `w-72 shrink-0` (and the API-key wrapper's `w-72`) with:

```
w-72 max-w-full shrink
```

(SelectTrigger line 1014, key wrapper line 1033, model column line 1069, custom-model Input line 1147, base-URL Input line 1164.)

- [ ] **Step 3: Render provider SelectItems from `PROVIDER_LABELS`**

Replace the hardcoded items (lines 1018-1021):

```tsx
<SelectContent>
  {Object.entries(PROVIDER_LABELS).map(([value, label]) => (
    <SelectItem key={value} value={value}>
      {label}
    </SelectItem>
  ))}
</SelectContent>
```

`PROVIDER_LABELS` already holds "OpenAI" / "OpenRouter" / "Google Gemini" (brand names, intentionally untranslated) and the translated `custom` label, so this removes the duplication flagged in the audit without translating brand names.

- [ ] **Step 4: Typecheck**

Run: `pnpm lint`
Expected: exit 0.

- [ ] **Step 5: Commit**

```bash
git add src/views/ModelsView.tsx
git commit -m "refactor(models): SettingRow for provider config; flexible widths at narrow window sizes"
```

---

### Task 8: Accessibility pass

**Files:**
- Modify: `src/views/SettingsView.tsx`, `src/views/TranscriptionsView.tsx`, `src/views/ModelsView.tsx`, `src/App.css`

- [ ] **Step 1: Truly disable the live-transcription sub-section (SettingsView)**

The wrapper div (line 726 pre-refactor) uses `opacity-50 pointer-events-none select-none`, which blocks clicks but not keyboard focus. React 19 supports the `inert` attribute; replace with:

```tsx
<div
  className={liveEnabled ? "" : "opacity-50 select-none"}
  inert={!liveEnabled}
>
```

(`inert` removes the subtree from focus order and click targets; drop `pointer-events-none` and `aria-disabled`.)

- [ ] **Step 2: Focus rings on the shortcut-capture buttons (SettingsView)**

Both inline `<button>`s (lines 908 and 927 pre-refactor) get these classes appended:

```
focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/60
```

- [ ] **Step 3: Keyboard expansion for transcription rows (TranscriptionsView)**

On the row div (line 206), add:

```tsx
role="button"
tabIndex={0}
aria-expanded={isExpanded}
onKeyDown={(e) => {
  if (e.target === e.currentTarget && (e.key === "Enter" || e.key === " ")) {
    e.preventDefault();
    toggleExpanded(item.id);
  }
}}
```

and append to its className string:

```
focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring/60
```

The `e.target === e.currentTarget` guard keeps Enter/Space working normally on the nested delete/copy buttons.

- [ ] **Step 4: Focus ring on the delete-model icon button (ModelsView, line ~688)**

Append to the button's className:

```
focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/60 rounded-md
```

- [ ] **Step 5: Sidebar nav focus state (App.css)**

After the `.nav-item.active` rule (line 255), add:

```css
.nav-item:focus-visible {
  outline: 2px solid var(--ring);
  outline-offset: -2px;
}
```

- [ ] **Step 6: Typecheck**

Run: `pnpm lint`
Expected: exit 0. (If `inert` errors under the installed React 19 types, use `inert={!liveEnabled || undefined}`.)

- [ ] **Step 7: Commit**

```bash
git add src/views/SettingsView.tsx src/views/TranscriptionsView.tsx src/views/ModelsView.tsx src/App.css
git commit -m "fix(a11y): keyboard focus, inert disabled section, visible focus rings"
```

---

### Task 9: Visual consistency pass

**Files:**
- Modify: `src/views/SettingsView.tsx`, `src/views/ModelsView.tsx`, `src/views/TranscriptionsView.tsx`, `src/views/UsageView.tsx`

- [ ] **Step 1: Same entrance animation on every view**

UsageView's root already has `animate-[fadeIn_0.3s_ease-out]` (line 469). Add the identical class to the root `<div>` returned by:
- `SettingsView` (line 600: `<div className="flex flex-col">` → `<div className="flex flex-col animate-[fadeIn_0.3s_ease-out]">`)
- `ModelsView` — locate the root with `grep -n "return (" src/views/ModelsView.tsx | head -3`, append the class to the outermost div's className
- `TranscriptionsView` — same approach

- [ ] **Step 2: Chart grid lines on tokens (UsageView lines 574-577)**

Replace the three `border-dashed border-white/10` with `border-dashed border-border` and the solid `border-white/15` with `border-border-hover`.

- [ ] **Step 3: Radius normalization (TranscriptionsView)**

Floating-surface radius is reserved for HUD/overlay panels; in-card containers use `rounded-xl`:
- line 276: `bg-surface-active rounded-2xl p-4` → `bg-surface-active rounded-xl p-4`
- line 285: `border border-dashed border-border rounded-2xl` → `border border-dashed border-border rounded-xl`

- [ ] **Step 4: Typecheck**

Run: `pnpm lint`
Expected: exit 0.

- [ ] **Step 5: Commit**

```bash
git add src/views/SettingsView.tsx src/views/ModelsView.tsx src/views/TranscriptionsView.tsx src/views/UsageView.tsx
git commit -m "style(ui): consistent view entrance, token-based chart grid, radius scale"
```

---

### Task 10: UsageView chart empty state

**Files:**
- Modify: `src/views/UsageView.tsx`

- [ ] **Step 1: Add the icon import**

Extend the existing lucide import with `BarChart3`.

- [ ] **Step 2: Render the empty state inside the chart card**

The chart card container (line 553) is `relative`. Directly after its header row (the `flex justify-between items-center mb-6` div ending line 564), add:

```tsx
{totalDuration === 0 && (
  <div className="absolute inset-0 z-20 flex flex-col items-center justify-center text-center rounded-xl bg-secondary/60 backdrop-blur-[2px]">
    <BarChart3 size={24} className="text-muted-dark mb-3" />
    <p className="text-muted text-sm">{t("usage.emptyTitle")}</p>
    <p className="text-muted-dark text-xs mt-1">{t("usage.emptyHint")}</p>
  </div>
)}
```

`totalDuration` is the existing aggregate used by the stat card (line 499); zero means the selected range has no transcription activity.

- [ ] **Step 3: Typecheck and key check**

Run: `pnpm lint && pnpm check:i18n`
Expected: both exit 0.

- [ ] **Step 4: Commit**

```bash
git add src/views/UsageView.tsx
git commit -m "feat(usage): empty state for the activity chart"
```

---

### Task 11: Final verification, docs, screenshots check

**Files:**
- Modify: `SIMPLEVOICE.md` (Recording Window section)
- Check only: `assets/screenshot-*.svg`, `README.md`

- [ ] **Step 1: Full check suite**

Run: `pnpm lint && pnpm check:i18n` and `cargo check` (cwd: `src-tauri/`)
Expected: all exit 0.

- [ ] **Step 2: Manual verification on macOS**

Run: `pnpm tauri dev` and walk this checklist:
1. Settings → Recording & Feedback shows "Move the recording bar" with a ⌘ keycap and a "Reset bar position" row; switch row absent on macOS.
2. With the overlay visible, hold ⌘ → the pill border glows amber; release → glow stops; drag while held moves the bar and the position survives an app restart.
3. Click Reset → bar returns top-center, success toast appears.
4. Switch app language to PL and DE → new strings translate.
5. Resize the main window to 800×600 → walk all four views; Models cloud tab inputs wrap instead of overflowing; transcription rows toggle with Enter/Space; tab through Settings — live sub-section is skipped while disabled, shortcut buttons show focus rings.
6. UsageView with no data in range (temporarily select a range with no usage if available) shows the empty state.

- [ ] **Step 3: README screenshot check**

The shipped changes do not alter UsageView's with-data rendering nor the overlay pill's default look (glow existed; only its trigger changed), so `assets/screenshot-usage.svg` / `assets/screenshot-overlay.svg` traces should be unaffected — confirm by eye against the running app. If any visual diff exists (e.g. chart grid-line shade), re-sync the SVG traces 1:1 in this task.

- [ ] **Step 4: Update SIMPLEVOICE.md**

In the "Recording Window" architecture section, append:

```markdown
Bar position: `data-tauri-drag-region` on the pill; macOS Cmd-hold toggles click-through (polling thread, also emits `recording-window-lock-status` for the amber glow), Linux/Windows use the lock toggle (tray + Settings). `reset_recording_window_position` restores the default top-center placement and clears `recording_window_has_custom_pos`.
```

- [ ] **Step 5: Commit and push**

```bash
git add SIMPLEVOICE.md
git commit -m "docs: recording-bar position controls in architecture notes"
git push origin main
```

---

## Out of scope (per spec)

Light theme, navigation redesign, new views, a macOS settings-side lock toggle. The audit items about select `bg-black` vs `bg-secondary` were judged intentional (controls inside `bg-secondary` cards use `bg-black`; controls on the page background use `bg-secondary`) and are left as-is.

**Re-verification outcomes recorded post-implementation:** spec theme 6's "consistent error presentation" needed no change (the existing inline-alert vs dialog split maps to recoverable vs blocking errors); the cloud-models empty state was already covered by the `usingFallbackModels` hint; theme 5 keeps `animate-in` utilities inside the shortcut-capture overlay (modal feedback, same system as the dialog primitives); theme 3's icon-only buttons keep native `title` + `aria-label` (Tooltip-primitive migration left as follow-up).

**SettingsView column breakpoints stay as `columns-1 lg:columns-2 2xl:columns-3`.** The spec asked for "a usable layout below lg"; measurement shows the existing single column is the usable layout there: at the 800 px minimum window the content area is ~512-736 px wide (sidebar 240/64 px + view padding), so two columns would be ~250 px each — too narrow for rows with switches and selects. The real narrow-window overflow risk was ModelsView's fixed `w-72` controls, fixed in Task 7, and Task 11 step 5 verifies all views at 800×600.
