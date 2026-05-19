import { ChevronDown } from "lucide-react";

export function SettingsView() {
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
              <select className="input w-full bg-black border-border rounded-md pl-4 pr-10 py-3 appearance-none cursor-pointer hover:border-muted transition-colors text-sm font-medium">
                <option>MacBook Pro Microphone</option>
                <option>External USB Mic</option>
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
      </div>
    </div>
  );
}
