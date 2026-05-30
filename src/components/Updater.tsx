import { useEffect, useState } from "react";
import { check, Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { Download, RefreshCw, AlertTriangle, CheckCircle, Info } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Progress } from "@/components/ui/progress";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";

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

  const busy = status === "downloading" || status === "installing";

  return (
    <Dialog
      open={isOpen}
      onOpenChange={(open) => {
        // Only allow dismissal in resting states — never mid-update.
        if (!open && (status === "available" || status === "error")) {
          setIsOpen(false);
        }
      }}
    >
      <DialogContent showCloseButton={false} className="sm:max-w-md">
        <DialogHeader className="flex-row items-center gap-3">
          <div className="flex size-10 shrink-0 items-center justify-center rounded-xl bg-accent text-white">
            <RefreshCw size={18} className={busy ? "animate-spin" : ""} />
          </div>
          <div className="flex flex-col gap-0.5">
            <DialogTitle>Update available</DialogTitle>
            {updateInfo && (
              <DialogDescription className="mono text-xs">
                Version {updateInfo.version}
              </DialogDescription>
            )}
          </div>
        </DialogHeader>

        {status === "available" && updateInfo?.body && (
          <div className="max-h-48 overflow-y-auto rounded-lg border border-border bg-background/60 p-3">
            <div className="mb-2 flex items-center gap-1.5 border-b border-border pb-1.5 text-[11px] font-medium tracking-wider text-muted uppercase">
              <Info size={12} />
              <span>What&apos;s new</span>
            </div>
            <p className="text-[13px] leading-relaxed whitespace-pre-wrap text-foreground/80">
              {updateInfo.body}
            </p>
          </div>
        )}

        {(status === "downloading" || status === "installing" || status === "completed") && (
          <div className="flex flex-col gap-2 py-1">
            <div className="flex justify-between text-[12px] font-medium">
              <span className="text-muted">
                {status === "downloading" && "Downloading update…"}
                {status === "installing" && "Installing…"}
                {status === "completed" && "Done — relaunching…"}
              </span>
              <span className="mono text-foreground">{progress}%</span>
            </div>
            <Progress value={progress} />
          </div>
        )}

        {status === "error" && (
          <Alert variant="destructive" className="border-danger/20 bg-danger/5">
            <AlertTriangle />
            <AlertTitle>Update failed</AlertTitle>
            <AlertDescription>{errorMsg || "An unexpected error occurred."}</AlertDescription>
          </Alert>
        )}

        <DialogFooter>
          {status === "available" && (
            <>
              <Button variant="ghost" onClick={() => setIsOpen(false)}>
                Skip
              </Button>
              <Button onClick={handleInstall}>
                <Download size={14} />
                Update now
              </Button>
            </>
          )}

          {status === "error" && (
            <Button variant="outline" onClick={() => setIsOpen(false)}>
              Close
            </Button>
          )}

          {busy && (
            <Button variant="ghost" disabled>
              Updating…
            </Button>
          )}

          {status === "completed" && (
            <span className="inline-flex items-center gap-1.5 text-xs font-medium text-success">
              <CheckCircle size={14} />
              Relaunching…
            </span>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
