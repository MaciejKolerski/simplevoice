// Builds the init-script installed via page.addInitScript. Must be fully
// self-contained when serialized: receives plain-JSON payload, no closures.
export function installTauriMock(payload) {
  const { fixtures, windowLabel } = payload;
  const listeners = new Map(); // event -> [callbackId]

  const respond = (cmd, args) => {
    switch (cmd) {
      case "load_config": return JSON.stringify(fixtures.config);
      case "check_permissions_status": return fixtures.permissions;
      case "get_usage_stats": return fixtures.usage;
      case "get_transcriptions": return fixtures.transcriptions;
      case "get_model_status": return { active: fixtures.models.modelsDir + "/" + fixtures.models.active, loading: null };
      case "get_active_model": return fixtures.models.modelsDir + "/" + fixtures.models.active;
      case "get_models_dir": return fixtures.models.modelsDir;
      case "scan_models": return fixtures.models.scan;
      case "list_cloud_models": return fixtures.models.cloud;
      case "list_audio_devices": return fixtures.devices;
      case "get_gpu_enabled": return true;
      case "has_secure_api_key": return false;
      case "has_last_recording_samples": return false;
      case "is_recording_window_locked_cmd": return true;
      default: return null; // all write-side commands succeed silently
    }
  };

  let nextCb = 1000;
  const internals = {
    metadata: {
      currentWindow: { label: windowLabel },
      currentWebview: { label: windowLabel, windowLabel },
    },
    transformCallback(cb) {
      const id = nextCb++;
      window[`_${id}`] = cb;
      return id;
    },
    async invoke(cmd, args = {}) {
      if (cmd === "plugin:event|listen") {
        const ev = args.event;
        if (!listeners.has(ev)) listeners.set(ev, []);
        listeners.get(ev).push(args.handler);
        return nextCb++; // event id
      }
      if (cmd === "plugin:event|unlisten") return null;
      if (cmd === "plugin:app|version") return "0.1.0";
      if (cmd === "plugin:app|name") return "Simplevoice";
      if (cmd.startsWith("plugin:")) return null; // updater check -> no update, etc.
      return respond(cmd, args);
    },
  };
  window.__TAURI_INTERNALS__ = internals;
  // @tauri-apps/api v2 event.js calls this synchronously in _unlisten() on unmount.
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
  window.__fireTauriEvent = (event, payloadData) => {
    for (const id of listeners.get(event) ?? []) {
      const fn = window[`_${id}`];
      if (fn) fn({ event, id: 0, payload: payloadData });
    }
  };
  try {
    localStorage.setItem("asr_engine", "local");
    localStorage.setItem("live_overlay_mode", "full");
    // Set shortcuts in localStorage (source of truth per SettingsView.tsx)
    localStorage.setItem("global_record_shortcut", "CommandOrControl+Shift+Space");
    localStorage.setItem("global_copy_shortcut", "CommandOrControl+Shift+C");
    localStorage.setItem("vad_enabled", "true");
    localStorage.setItem("recording_window_mode", "always");
  } catch {}
}
