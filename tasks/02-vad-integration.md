# Stage 2: Voice Activity Detection (VAD)

**Goal:** Prevent STT engine "hallucinations" by automatically trimming silence and triggering transcriptions only when speech is detected.

## Tasks:
- [ ] **Dependencies:** Add a lightweight VAD library (e.g., `webrtc-vad` bindings or implement a simple energy-based threshold gate).
- [ ] **Audio Pipeline Integration:** Intercept the PCM chunks in the `cpal` stream callback before pushing them to the main ring buffer.
- [ ] **Silence Detection:** Implement logic to track consecutive frames of silence.
- [ ] **Auto-Stop Logic:** If silence exceeds a specific threshold (e.g., 1.5 seconds) AND the user was previously speaking, automatically trigger the `stop_recording` flow and send an event to the frontend to begin processing.
- [ ] **Settings Integration:** Link the VAD logic to the boolean toggle in the Settings View.
