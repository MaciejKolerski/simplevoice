import { useEffect, useState } from "react";
import { ChevronDown, Trash2 } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";

export function SettingsView() {
  const [devices, setDevices] = useState<string[]>([]);
  const [selectedDevice, setSelectedDevice] = useState<string>("");
  const [clearing, setClearing] = useState(false);
  const [storageMessage, setStorageMessage] = useState<string | null>(null);
  const [showConfirmModal, setShowConfirmModal] = useState(false);

  const handleClearCache = async () => {
    setClearing(true);
    setStorageMessage(null);
    try {
      const msg = await invoke<string>("clear_app_files");
      setStorageMessage(msg);
      setTimeout(() => setStorageMessage(null), 5000);
    } catch (err) {
      console.error(err);
      setStorageMessage(`Error: ${err}`);
      setTimeout(() => setStorageMessage(null), 5000);
    } finally {
      setClearing(false);
    }
  };

  useEffect(() => {
    const loadDevices = async () => {
      try {
        const list = await invoke<string[]>("list_audio_devices");
        setDevices(list);
        
        const saved = localStorage.getItem("selected_audio_device");
        if (saved && list.includes(saved)) {
          setSelectedDevice(saved);
          await invoke("set_selected_device", { device: saved });
        } else {
          setSelectedDevice("default");
          await invoke("set_selected_device", { device: null });
        }
      } catch (err) {
        console.error("Failed to load audio devices:", err);
      }
    };
    loadDevices();
  }, []);

  const handleDeviceChange = async (val: string) => {
    setSelectedDevice(val);
    try {
      if (val === "default") {
        localStorage.removeItem("selected_audio_device");
        await invoke("set_selected_device", { device: null });
      } else {
        localStorage.setItem("selected_audio_device", val);
        await invoke("set_selected_device", { device: val });
      }
    } catch (err) {
      console.error("Failed to set selected device:", err);
    }
  };

  return (
    <div className="flex flex-col">
      <div className="mb-6">
        <h1 className="m-0 text-2xl font-medium text-white tracking-tight">
          Preferences
        </h1>
      </div>

      <div className="w-full">
        <h2 className="mt-0 mb-4 text-base text-white font-medium">
          Audio Processing
        </h2>
        <div className="border border-border rounded-xl overflow-hidden bg-secondary mb-10">
          <div className="flex flex-col p-6 border-b border-border">
            <label className="text-fg font-medium mb-3 block">
              Input Device
            </label>
            <div className="relative w-full">
              <select
                value={selectedDevice}
                onChange={(e) => handleDeviceChange(e.target.value)}
                className="input w-full bg-black border-border rounded-md pl-4 pr-10 py-3 appearance-none cursor-pointer hover:border-muted transition-colors text-sm font-medium"
              >
                <option value="default">Default System Microphone</option>
                {devices.map((device, idx) => (
                  <option key={idx} value={device}>
                    {device}
                  </option>
                ))}
              </select>
              <div className="absolute right-3 top-1/2 -translate-y-1/2 pointer-events-none text-muted">
                <ChevronDown size={18} />
              </div>
            </div>
          </div>
          <div className="flex justify-between items-center p-6">
            <div>
              <div className="text-fg font-medium mb-1">
                Voice Activity Detection (VAD)
              </div>
              <div className="text-muted text-[13px]">
                Automatically pause processing when you stop speaking.
              </div>
            </div>
            <label className="toggle">
              <input type="checkbox" defaultChecked />
              <span className="toggle-bg"></span>
            </label>
          </div>
        </div>

        <h2 className="mb-4 text-base text-white font-medium">Shortcuts</h2>
        <div className="border border-border rounded-xl overflow-hidden bg-secondary mb-10">
          <div className="flex justify-between items-center p-6 border-b border-border">
            <div>
              <div className="text-fg font-medium mb-1">
                Global Record Toggle
              </div>
              <div className="text-muted text-[13px]">
                Start/stop recording from anywhere.
              </div>
            </div>
            <div className="inline-flex items-center px-3 py-1.5 rounded text-xs font-mono font-medium bg-surface-active text-muted border border-border">
              Cmd + Shift + Space
            </div>
          </div>
          <div className="flex justify-between items-center p-6">
            <div>
              <div className="text-fg font-medium mb-1">Quick Copy</div>
              <div className="text-muted text-[13px]">
                Copy last transcription to clipboard.
              </div>
            </div>
            <div className="inline-flex items-center px-3 py-1.5 rounded text-xs font-mono font-medium bg-surface-active text-muted border border-border">
              Cmd + Shift + C
            </div>
          </div>
        </div>

        <h2 className="mb-4 text-base text-white font-medium">General</h2>
        <div className="border border-border rounded-xl overflow-hidden bg-secondary mb-10">
          <div className="flex justify-between items-center p-6 border-b border-border">
            <div>
              <div className="text-fg font-medium mb-1">Launch at Login</div>
              <div className="text-muted text-[13px]">
                Start SimpleVoice automatically when you log in.
              </div>
            </div>
            <label className="toggle">
              <input type="checkbox" />
              <span className="toggle-bg"></span>
            </label>
          </div>
          <div className="flex justify-between items-center p-6">
            <div>
              <div className="text-fg font-medium mb-1">Menu Bar Icon</div>
              <div className="text-muted text-[13px]">
                Show app icon in the macOS menu bar.
              </div>
            </div>
            <label className="toggle">
              <input type="checkbox" defaultChecked />
              <span className="toggle-bg"></span>
            </label>
          </div>
        </div>

        <h2 className="mb-4 text-base text-white font-medium">Storage</h2>
        <div className="border border-border rounded-xl overflow-hidden bg-secondary mb-10">
          <div className="flex flex-col p-6">
            <div className="flex justify-between items-center w-full">
              <div>
                <div className="text-fg font-medium mb-1">Clear Cache & Recordings</div>
                <div className="text-muted text-[13px]">
                  Remove all temporary recordings and cached audio files from disk.
                </div>
              </div>
              <button
                onClick={() => setShowConfirmModal(true)}
                disabled={clearing}
                className="inline-flex items-center justify-center gap-2 border border-red-500/20 bg-red-500/10 hover:bg-red-500/20 text-red-400 px-3 sm:px-4 py-2 text-xs font-medium rounded-md transition-all duration-200 cursor-pointer disabled:opacity-50 hover:translate-y-[-1px] active:translate-y-0 shrink-0 whitespace-nowrap"
              >
                <Trash2 size={13} className="shrink-0" />
                <span className="hidden sm:inline">
                  {clearing ? "Clearing..." : "Clear Files"}
                </span>
              </button>
            </div>
            {storageMessage && (
              <div className="text-xs text-emerald-400 font-medium mt-3">
                {storageMessage}
              </div>
            )}
          </div>
        </div>
      </div>

      {showConfirmModal && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm transition-all duration-300">
          <div className="bg-secondary border border-border rounded-xl p-6 max-w-sm w-full mx-4 shadow-2xl animate-in fade-in zoom-in-95 duration-200">
            <h3 className="text-lg font-medium text-white mb-2">
              Clear Cache & Recordings?
            </h3>
            <p className="text-muted text-[13px] mb-6 leading-relaxed">
              This will permanently delete all temporary `.wav` files and empty the active in-memory recording buffer. This action cannot be undone.
            </p>
            <div className="flex justify-end gap-3">
              <button
                onClick={() => setShowConfirmModal(false)}
                className="btn btn-outline px-4 py-2 text-xs rounded-md"
              >
                Cancel
              </button>
              <button
                onClick={() => {
                  setShowConfirmModal(false);
                  handleClearCache();
                }}
                className="btn bg-red-600 hover:bg-red-500 text-white border-0 px-4 py-2 text-xs font-semibold rounded-md"
              >
                Confirm Delete
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
