import { useEffect, useState } from "react";
import { check, Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { Download, RefreshCw, AlertTriangle, CheckCircle, Info } from "lucide-react";

export function Updater() {
  const [updateInfo, setUpdateInfo] = useState<Update | null>(null);
  const [isOpen, setIsOpen] = useState(false);
  const [status, setStatus] = useState<"idle" | "checking" | "available" | "downloading" | "installing" | "completed" | "error">("idle");
  const [progress, setProgress] = useState(0);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);

  useEffect(() => {
    const checkForUpdates = async () => {
      setStatus("checking");
      try {
        console.log("[Updater] Checking for updates...");
        const update = await check();
        if (update) {
          console.log(`[Updater] Update found: ${update.version}`);
          setUpdateInfo(update);
          setStatus("available");
          setIsOpen(true);
        } else {
          console.log("[Updater] App is up to date.");
          setStatus("idle");
        }
      } catch (err) {
        console.error("[Updater] Error checking for updates:", err);
        setStatus("error");
        setErrorMsg(err instanceof Error ? err.message : String(err));
      }
    };

    // Check for updates on startup (with a slight delay to let the app load smoothly)
    const timer = setTimeout(() => {
      checkForUpdates();
    }, 3000);

    return () => clearTimeout(timer);
  }, []);

  const handleInstall = async () => {
    if (!updateInfo) return;

    setStatus("downloading");
    setProgress(0);
    setErrorMsg(null);

    try {
      console.log("[Updater] Downloading and installing update...");
      
      let contentLength: number | undefined = 0;
      let downloaded = 0;

      await updateInfo.downloadAndInstall((event) => {
        switch (event.event) {
          case "Started":
            contentLength = event.data.contentLength;
            setStatus("downloading");
            console.log(`[Updater] Download started, total: ${contentLength} bytes`);
            break;
          case "Progress":
            downloaded += event.data.chunkLength;
            if (contentLength) {
              const percentage = Math.round((downloaded / contentLength) * 100);
              setProgress(percentage);
            }
            break;
          case "Finished":
            console.log("[Updater] Download finished, installing...");
            setStatus("installing");
            break;
        }
      });

      setStatus("completed");
      console.log("[Updater] Update completed, relaunching app...");
      
      // Wait 1.5 seconds to show completion state then relaunch
      setTimeout(async () => {
        try {
          await relaunch();
        } catch (relaunchErr) {
          console.error("[Updater] Failed to relaunch application automatically:", relaunchErr);
          setErrorMsg("Update downloaded. Please restart the app manually.");
          setStatus("error");
        }
      }, 1500);

    } catch (err) {
      console.error("[Updater] Error installing update:", err);
      setStatus("error");
      setErrorMsg(err instanceof Error ? err.message : String(err));
    }
  };

  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 backdrop-blur-md transition-all duration-300">
      <div className="bg-[#18181b] border border-[#27272a] rounded-xl p-6 max-w-md w-full mx-4 shadow-2xl animate-in fade-in zoom-in-95 duration-200 text-white">
        
        {/* Header */}
        <div className="flex items-center gap-3 mb-4">
          <div className="p-2 rounded-lg bg-blue-500/10 text-blue-400">
            <RefreshCw size={20} className={status === "downloading" || status === "installing" ? "animate-spin" : ""} />
          </div>
          <div>
            <h3 className="text-md font-semibold text-white">
              New Version Available!
            </h3>
            {updateInfo && (
              <p className="text-[12px] text-gray-400 mt-0.5">
                Version {updateInfo.version}
              </p>
            )}
          </div>
        </div>

        {/* Content / Release Notes */}
        <div className="mb-6">
          {status === "available" && updateInfo?.body && (
            <div className="bg-[#09090b] rounded-lg p-3 border border-[#27272a] max-h-48 overflow-y-auto">
              <div className="flex items-center gap-1.5 text-[11px] font-medium text-gray-400 mb-2 border-b border-[#27272a] pb-1">
                <Info size={12} />
                <span>Changelog:</span>
              </div>
              <p className="text-gray-300 text-[12px] whitespace-pre-wrap leading-relaxed">
                {updateInfo.body}
              </p>
            </div>
          )}

          {/* Progress Section */}
          {(status === "downloading" || status === "installing" || status === "completed") && (
            <div className="space-y-3 py-2">
              <div className="flex justify-between text-[12px] font-medium text-gray-300">
                <span>
                  {status === "downloading" && "Downloading update..."}
                  {status === "installing" && "Installing..."}
                  {status === "completed" && "Completed! Relaunching..."}
                </span>
                <span>{progress}%</span>
              </div>
              <div className="w-full bg-gray-800 rounded-full h-2.5 overflow-hidden">
                <div 
                  className="bg-blue-500 h-full rounded-full transition-all duration-300 ease-out"
                  style={{ width: `${progress}%` }}
                ></div>
              </div>
            </div>
          )}

          {/* Error Section */}
          {status === "error" && (
            <div className="bg-red-500/10 border border-red-500/20 rounded-lg p-3 text-[12px] text-red-400 flex items-start gap-2">
              <AlertTriangle size={16} className="shrink-0 mt-0.5" />
              <div>
                <span className="font-semibold block mb-0.5">Update Error</span>
                <span className="opacity-90">{errorMsg || "An unexpected error occurred."}</span>
              </div>
            </div>
          )}
        </div>

        {/* Action Buttons */}
        <div className="flex justify-end gap-3 border-t border-[#27272a] pt-4">
          {status === "available" && (
            <>
              <button
                onClick={() => setIsOpen(false)}
                className="px-4 py-2 text-xs font-medium text-gray-400 hover:text-white transition-colors cursor-pointer"
              >
                Skip
              </button>
              <button
                onClick={handleInstall}
                className="bg-blue-600 hover:bg-blue-500 text-white px-4 py-2 text-xs font-semibold rounded-md shadow-md hover:shadow-blue-500/20 transition-all flex items-center gap-1.5 cursor-pointer"
              >
                <Download size={14} />
                Update Now
              </button>
            </>
          )}

          {status === "error" && (
            <button
              onClick={() => setIsOpen(false)}
              className="bg-[#27272a] hover:bg-[#3f3f46] text-white px-4 py-2 text-xs font-semibold rounded-md transition-colors cursor-pointer"
            >
              Close
            </button>
          )}

          {(status === "downloading" || status === "installing") && (
            <button
              disabled
              className="px-4 py-2 text-xs font-medium text-gray-500 cursor-not-allowed"
            >
              Updating...
            </button>
          )}

          {status === "completed" && (
            <div className="flex items-center gap-1.5 text-xs text-green-400 font-medium">
              <CheckCircle size={14} />
              Relaunching...
            </div>
          )}
        </div>

      </div>
    </div>
  );
}
