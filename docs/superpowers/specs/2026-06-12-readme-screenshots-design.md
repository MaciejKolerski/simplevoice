# README Screenshots Refresh · Design Spec

**Date:** 2026-06-12
**Repo:** `simplevoice` (the desktop app)
**Branch:** `feat/readme-screenshots`

## Goal

Replace the README's two hand-maintained SVG mockups with five fresh PNG captures of the REAL app UI, grouped in an HTML table (user-specified layout: 2 + 2 + colspan banner), and apply targeted freshness fixes found while analyzing the README.

## Method — real frontend, fixture data (approved)

Screenshots come from the app's actual React components and CSS, rendered by the Vite dev server in headless Chromium (Playwright), with the Tauri IPC layer mocked. This is literally the UI the user ships — same code, same self-hosted Inter/JetBrains Mono fonts — with deterministic, presentable data. No Rust build, no app-source changes.

### Capture tooling

- `scripts/readme-shots/capture.mjs` (Node, ESM) + `scripts/readme-shots/fixtures.mjs`.
- devDependency: `playwright` (chromium). New package script: `"shots": "node scripts/readme-shots/capture.mjs"` (boots `pnpm dev` itself if the server is not already up, or expects it running — implementation picks one and documents it).
- `page.addInitScript` installs:
  - `window.__TAURI_INTERNALS__.invoke(cmd, args)` → fixture switch (inventory of every `invoke()` used by the views is step 1 of implementation; every command must be covered — an unmocked command means a broken section and fails QC).
  - mocked `listen`/event API → no-op unsubscribes; the recording-overlay capture drives `audio-amplitude` events through it to pose the live waveform.
  - `localStorage` seeds (`asr_engine=local`, UI language `en`).
- Viewport 1280×800, `deviceScaleFactor: 2`, dark app theme as-is.
- Window chrome added at capture time via injected styles only: 12 px rounded corners + overflow hidden on `#root`, soft drop shadow, three macOS traffic dots (#ff5f57/#febc2e/#28c840, 12 px, standard offsets) absolutely positioned in the TitleBar's reserved 64 px macOS zone (platform fixture = `macos`). Screenshot with `omitBackground: true` → transparent-alpha PNGs that sit well on GitHub light and dark themes.

### Panels & fixtures

| File | View | Fixture highlights |
| --- | --- | --- |
| `assets/readme/usage.png` | Usage | Totals 42 m 13 s / 48,210; previous-week data ⇒ both trends +12%; Mon–Sun daily seconds shaped to the established chart silhouette (relative heights ≈ 38/62/28/78/46/90/58, today = Sun); active model `parakeet-tdt-0.6b-v3.onnx` (the app renders the real filename — kept honest), status “Running locally”. |
| `assets/readme/models.png` | Models | Catalog with Parakeet TDT v3 downloaded + active; a Whisper GGML entry and one more catalog row with realistic sizes; local engine selected. Exact shape depends on the ModelsView invoke inventory. |
| `assets/readme/transcriptions.png` | Transcriptions | ~6 rows over the last week: the video’s sentences (“Ship the release notes today…”, “Sounds great — let’s lock Friday…”) plus neutral entries; realistic durations/word counts/dates. |
| `assets/readme/settings.png` | Settings | Default macOS config: shortcuts ⌘⇧Space / ⌘⇧C, sound feedback on, recording-overlay mode “Show During Recording”, recording-bar position controls visible. |
| `assets/readme/recording.png` | Recording overlay (banner) | REAL `RecordingWindowView` posed mid-speech (amplitude events ⇒ waveform alive, timer 0:07), captured with alpha, then composited in a staging page over a neutral dark backdrop with a generic “Notes — Untitled” window (same concept as the current `screenshot-recording.svg`). Wide aspect (~2400×700 @2x) for the colspan row. |

Honesty rule (unchanged from the landing/video work): all values are illustrative product UI consistent with existing brand assets; nothing presented as real user metrics; the overlay banner’s backdrop is staging, the overlay itself is live app code.

### README changes

- Replace the current centered two-`<img>` block with `## Screenshots` + the approved table: row 1 Usage | Models, row 2 Transcriptions | Settings, row 3 `colspan="2"` recording banner; each cell `<td align="center"><img …/><br/><sub>EN caption</sub></td>`.
- Keep `assets/screenshot-*.svg` files in the repo (still referenced by brand work) but no longer from the README.
- Freshness fixes from analysis: version badge vs `tauri.conf.json`; confirm the Configuration section mentions the recording-bar position controls (recent feature); alt texts updated. No broader rewrite.

### Verification / acceptance

1. Invoke inventory complete: `grep -n "invoke(" src/views/*.tsx src/components/**/*.tsx src/context/*.tsx` — every command either mocked or proven unused by the captured views.
2. Each PNG visually reviewed (controller + user): correct fonts (no fallback serif), no empty/error states, no scrollbars, traffic dots aligned, data internally consistent (chart sum ↔ totals ↔ trend).
3. README HTML table renders correctly (GitHub-flavored preview); image paths valid; total added asset weight reasonable (target < 2.5 MB across five PNGs; downscale to @1.5x if exceeded).
4. `pnpm lint` passes (repo convention) — script files included.
5. No changes to `src/` of the app.

### Risks

- Views may call Tauri plugin internals beyond plain `invoke` (e.g. `tauri-plugin-sql` channels) — mitigated by the inventory step; worst case a view needs one extra mocked surface (`window.__TAURI_INTERNALS__.transformCallback` etc.).
- React StrictMode double-invokes effects in dev — fixtures must be idempotent.
- GitHub caches README images aggressively — new filenames (`assets/readme/*.png`) avoid stale-cache confusion entirely.
