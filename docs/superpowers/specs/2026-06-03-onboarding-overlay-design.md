# Onboarding Overlay — Design Spec

Date: 2026-06-03
Status: Approved for planning
Scope: First-run guided tour for the Simplevoice desktop app (main window only)

## 1. Goal

New users find Simplevoice hard to grasp: it depends on permissions, a downloaded
(or cloud) speech-to-text model, a global shortcut, and several preferences before
it does anything useful. This feature adds a first-run, step-by-step guided tour
rendered as a **spotlight overlay on the real application window**: it highlights
actual UI elements, explains "what / how / why", and walks the user through getting
the app working.

It is a coachmark/spotlight tour over the live UI — not a separate wizard screen and
not a generic tour library.

## 2. Decisions (from brainstorming)

- **Style:** spotlight overlay on the real window, highlighting real elements.
- **Behaviour:** hybrid — mostly explain + highlight with a "Next" button, but key
  steps (permissions, getting a working model) gate "Next" until the action is done.
- **Scope:** extended — ~8 steps (welcome, permissions, engine/model, record
  shortcut, language, recording options, live test, done).
- **Control:** "Skip" available at all times; **no replay entry point in the UI**
  (a first-run flag is enough). Re-adding replay later is trivial if wanted.
- **Approach:** custom overlay driven by a dedicated `OnboardingProvider`
  (no third-party tour dependency), to match the custom Tailwind v4 design system
  and the project's production quality bar.

## 3. Architecture & Components

New directory `src/components/onboarding/`:

| File | Responsibility |
|------|----------------|
| `OnboardingProvider.tsx` | Context + state (`isActive`, `currentStep`, `next/back/skip/finish`). Detects first run, builds the platform-specific step list, persists the completion flag. |
| `OnboardingOverlay.tsx` | Visual layer: spotlight mask, highlight ring, tooltip card (title, "what/how/why" body, step counter, Back/Next/Skip buttons). |
| `useSpotlight.ts` | Hook tracking the highlighted element's position (`getBoundingClientRect`); recomputes on resize, view change, and async appearance of the target. |
| `steps.tsx` | Step definitions via a pure `buildSteps(platform, …)` function returning the step array. Unit-testable. |

### Integration with existing code

- Provider wraps `App` in `main.tsx` — **only in the main-window branch**; the
  `recording_window` branch does not get it.
- Overlay renders inside `App`, above content. Tour overlay sits at `z-40`; the
  existing recording HUD stays at `z-50`.
- Existing elements get additive `data-tour="..."` attributes (in `Sidebar.tsx`,
  `ModelsView.tsx`, `SettingsView.tsx`). These are the only edits to current views
  and are purely additive.
- View switching reuses the existing `navigate-to-view` CustomEvent that `App`
  already listens for (App.tsx:129). A step declares its `view`; the Provider
  dispatches `navigate-to-view` when entering the step.

### Step shape

```ts
interface OnboardingStep {
  id: string;
  view: "usage" | "models" | "transcriptions" | "settings";
  target?: string;          // data-tour selector; absent = centered card
  title: string;
  body: ReactNode;          // "what / how / why"
  placement?: "auto" | "top" | "bottom" | "left" | "right";
  gate?: () => boolean | Promise<boolean>;  // hybrid: blocks "Next" until satisfied
  gatePollMs?: number;      // e.g. poll is_recording_allowed
}
```

## 4. First-run detection & state flow

- Source of truth: `onboarding_completed: true` in `config.json` (the canonical
  store per SIMPLEVOICE.md), read through `ConfigContext`.
- On main-window start: once `ConfigContext` has loaded and the flag is **absent**,
  the Provider starts the tour, protected by a `ref` guard against React StrictMode
  double-mount (see SIMPLEVOICE.md gotcha).
- "Skip" or finishing the last step → `updateConfig("onboarding_completed", true)`.
  After that it never shows again.
- Safety: if persisting the flag fails, log the error but still close the tour —
  never block the app.

## 5. Spotlight mechanics

- **Mask:** four dimmed divs (top/bottom/left/right of the target rect),
  `bg-black/60 backdrop-blur-[1px]`. They provide both the dimming **and** the
  click-blocking outside the highlight, while the center (the real element) stays
  fully clickable. This is more robust than a `box-shadow` trick because it leaves
  a real interactive "hole".
- **Ring:** a `2px` border with a soft glow around the target rect, corner radius
  matched to the element, animated entrance.
- **Tooltip card:** auto-placed (above/below/beside depending on available space),
  with a "Step 3/8" counter and Back/Next/Skip buttons. With no `target`
  (welcome, finish) the card is centered over a full dim.
- **Position recompute:** `ResizeObserver` on the element + window `resize` listener
  + recompute on step change. If the element does not exist yet (async model list
  loading), poll briefly until it appears; meanwhile show the card centered in a
  "preparing…" state.

## 6. Step list (8)

| # | View | Highlight (`data-tour`) | "What/How/Why" content | Gate (hybrid) |
|---|------|-------------------------|------------------------|---------------|
| 1 | usage | — (centered) | Welcome. What Simplevoice is: local, private speech-to-text. How it works: shortcut → speak → text auto-pastes. | none |
| 2 | settings | `permissions-section` (macOS) / `shortcuts-section` (Linux) | Permissions: why microphone and Accessibility (auto-paste) are needed. The "Grant" buttons in this section are the real ones. | **macOS:** mic + accessibility granted (poll `check_permissions_status` every 2 s). **Linux:** info only, no gate. **Windows:** step omitted |
| 3 | models | `engine-tabs` + `models-card` | Choose engine: Local (download a model) or Cloud (API key). "Click Get to download, then Load" / "enter your key". | `recordingReady()` (poll every 1.5 s) — see below. Works for local (model loaded) and cloud (key set) |

`recordingReady()` reproduces the backend's `is_recording_allowed` check on the
frontend (that Rust function is internal, not an invokable command). It reads the
selected engine from `localStorage` and:

- **Local engine:** `get_model_status()` returns `active != null` (a model is loaded).
- **Cloud engine:** `has_secure_api_key(provider) === true` for the selected provider.

Both `get_model_status` and `has_secure_api_key` are existing, registered Tauri
commands.
| 4 | settings | `record-shortcut` | Global start/stop shortcut. A default already exists; click to change it. | none (informational) |
| 5 | settings | `language-select` | Transcription language — when to use "Auto" vs forcing a specific language. | none |
| 6 | settings | `recording-section` | Options: VAD (auto-stop on silence), sound effects, pause system audio. What each does and when to enable it. | none |
| 7 | usage | — (mask removed) | Live test: "Press your shortcut and say a sentence." Mask removed so the recording HUD is visible. "I'll do this later" button. | listen for `transcription-added` event → success (soft gate — skippable) |
| 8 | usage | `sidebar` | Done. Short recap + where to find things (Usage / Models / Transcriptions / Settings in the sidebar). "Finish" button. | none |

### `data-tour` additions (purely additive)

- `Sidebar.tsx` → `sidebar`
- `ModelsView.tsx` → `engine-tabs`, `models-card`
- `SettingsView.tsx` → `permissions-section`, `shortcuts-section`, `record-shortcut`,
  `language-select`, `recording-section`

## 7. Platform adaptation & error handling

- **`buildSteps(platform)`** filters/varies steps: step 2 has three variants
  (macOS gated / Linux info / Windows omitted); the rest are shared. Cloud vs Local
  is handled by the single step 3 via the `is_recording_allowed` gate.
- **Element not found** (async model list load): centered "preparing…" card, poll
  until the target appears, then snap to the spotlight.
- **Resize / small window:** recompute position; the card flips side when there is
  not enough room.
- **StrictMode (dev):** `ref` guard prevents a double start (per SIMPLEVOICE.md).
- **Test step vs HUD:** tour overlay at `z-40`, existing HUD at `z-50`; in step 7 the
  mask is removed and only a small instruction bar shows at the top, so the HUD is
  clean and visible. The global shortcut works (OS level) even with the overlay
  active.
- **No emojis** in content or code; copy is in English (consistent with the brand
  book and existing UI).

## 8. Testing & verification

- `steps.tsx` keeps logic in a **pure `buildSteps` function** — easy to unit-test.
- Verification: `pnpm lint` (required before commit) + manual `pnpm tauri dev` with
  the `onboarding_completed` flag cleared (full run-through, skip, and gates on the
  real window).
- The project has no JS test runner today — **not adding** vitest just for this
  (scope creep). Logic stays in a pure function; a small `buildSteps` unit test plus
  vitest setup can be added later as a separate, optional step if wanted.

## 9. Out of scope

- Replay entry point in the UI (decided against; first-run flag only).
- Adding a JS test runner.
- Changes to backend Rust commands. The tour only reads existing, registered
  commands (`check_permissions_status`, `get_model_status`, `has_secure_api_key`)
  and listens for the existing `transcription-added` window event. The internal
  `is_recording_allowed` Rust function is reproduced on the frontend rather than
  newly exposed.
