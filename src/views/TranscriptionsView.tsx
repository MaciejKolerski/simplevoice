import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ChevronDown, History, Trash2, Copy, Loader2 } from "lucide-react";
import { toast } from "sonner";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { HistoryAudioPlayer } from "@/components/HistoryAudioPlayer";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";

interface TranscriptionItem {
  id: string;
  timestamp: string;
  date: string;
  text: string;
  model: string;
  wav_path?: string;
  duration_sec?: number;
}

export function TranscriptionsView() {
  const { t } = useTranslation();
  const [history, setHistory] = useState<TranscriptionItem[]>([]);
  const [offset, setOffset] = useState(0);
  const [hasMore, setHasMore] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [showConfirmModal, setShowConfirmModal] = useState(false);
  const [showDeleteModal, setShowDeleteModal] =
    useState<TranscriptionItem | null>(null);
  const [isDeleting, setIsDeleting] = useState<string | null>(null);
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  const loadHistory = async (reset = false) => {
    const newOffset = reset ? 0 : offset;
    try {
      const data = await invoke<TranscriptionItem[]>("get_transcriptions", {
        limit: 20,
        offset: newOffset,
      });
      if (reset) {
        setHistory(data);
        setOffset(data.length);
      } else {
        setHistory((prev) => [...prev, ...data]);
        setOffset((prev) => prev + data.length);
      }
      setHasMore(data.length === 20);
    } catch (err) {
      console.error("Failed to load history:", err);
    } finally {
      if (reset) setLoading(false);
    }
  };

  useEffect(() => {
    loadHistory(true);

    const handleTranscriptionAdded = () => {
      loadHistory(true);
    };
    window.addEventListener("transcription-added", handleTranscriptionAdded);
    return () => {
      window.removeEventListener(
        "transcription-added",
        handleTranscriptionAdded,
      );
    };
  }, []);

  const handleCopy = async (text: string) => {
    try {
      await navigator.clipboard.writeText(text);
      toast.success(t("transcriptions.copiedToClipboard"));
    } catch (err) {
      console.error("Failed to copy text:", err);
      toast.error(t("transcriptions.copyFailed"));
    }
  };

  const handleClearHistory = () => {
    setShowConfirmModal(true);
  };

  const handleConfirmClearHistory = async () => {
    setShowConfirmModal(false);
    try {
      await invoke("clear_history_cmd");

      setHistory([]);
      setOffset(0);
      setHasMore(true);
      window.dispatchEvent(
        new CustomEvent("transcription-added", {
          detail: { source: "history" },
        }),
      );
      toast.success(t("transcriptions.historyCleared"));
    } catch (err) {
      console.error("Failed to clear history:", err);
      toast.error(t("transcriptions.clearFailed"));
    }
  };

  const deleteItem = (item: TranscriptionItem) => {
    setShowDeleteModal(item);
  };

  const handleConfirmDelete = async () => {
    if (!showDeleteModal) return;
    const item = showDeleteModal;
    setShowDeleteModal(null);

    setIsDeleting(item.id);
    try {
      await invoke("delete_transcription_cmd", {
        id: item.id,
        path: item.wav_path,
      });

      await loadHistory(true); // refresh from start after delete
      window.dispatchEvent(
        new CustomEvent("transcription-added", {
          detail: { source: "history" },
        }),
      );
    } catch (err) {
      console.error("Failed to delete item:", err);
      toast.error(t("transcriptions.deleteFailed"));
    } finally {
      setIsDeleting(null);
    }
  };

  const loadMore = async () => {
    if (loadingMore || !hasMore) return;
    setLoadingMore(true);
    await loadHistory(false);
    setLoadingMore(false);
  };

  const toggleExpanded = (id: string) => {
    setExpandedId(expandedId === id ? null : id);
  };

  return (
    <div className="flex flex-col w-full animate-[fadeIn_0.3s_ease-out]">
      <div className="flex justify-between items-end gap-4 mb-6">
        <div>
          <h1 className="m-0 text-2xl font-medium text-white tracking-tight">
            {t("transcriptions.title")}
          </h1>
          <p className="text-xs text-muted mt-1 leading-normal">
            {t("transcriptions.subtitle")}
          </p>
        </div>
        {history.length > 0 && (
          <Button
            variant="outline"
            size="sm"
            onClick={handleClearHistory}
            className="shrink-0 text-danger hover:text-danger hover:border-danger/40 hover:bg-danger/5"
          >
            <Trash2 size={13} />
            {t("transcriptions.clearHistory")}
          </Button>
        )}
      </div>

      {loading ? (
        // Skeleton rather than a spinner: the list has a known shape, so hold
        // its place instead of reflowing once rows arrive.
        <div
          role="status"
          aria-label={t("common.loading")}
          className="border border-border rounded-xl overflow-hidden bg-secondary"
        >
          {[0, 1, 2].map((i) => (
            <div
              key={i}
              className="flex flex-col gap-2.5 p-5 border-b border-border last:border-b-0"
            >
              <div className="flex gap-2.5">
                <div className="h-3 w-28 rounded bg-surface-active" />
                <div className="h-3 w-20 rounded bg-surface-active" />
              </div>
              <div className="h-3 w-full max-w-xl rounded bg-surface-active" />
              <div className="h-3 w-2/3 max-w-md rounded bg-surface-active" />
            </div>
          ))}
        </div>
      ) : history.length === 0 ? (
        <div className="flex flex-col items-center justify-center p-12 text-center border border-dashed border-border rounded-xl bg-secondary">
          <div className="flex size-14 items-center justify-center rounded-full bg-surface-active text-muted mb-4">
            <History size={26} />
          </div>
          <h3 className="text-white font-medium mb-2">
            {t("transcriptions.emptyTitle")}
          </h3>
          <p className="text-muted text-sm max-w-md mb-2 leading-relaxed">
            {t("transcriptions.emptyBody")}
          </p>
        </div>
      ) : (
        <div className="border border-border rounded-xl overflow-hidden bg-secondary">
          {history.map((item) => {
            const isExpanded = expandedId === item.id;
            return (
              // Not role="button": the row holds its own Delete and Copy
              // buttons, and nesting controls inside a control is invalid ARIA.
              // The chevron is the keyboard toggle; the row click is mouse-only.
              <div
                key={item.id}
                className={`group flex flex-col p-5 transition-colors hover:bg-surface-hover border-b border-border last:border-b-0 cursor-pointer ${
                  isExpanded ? "bg-surface-hover" : ""
                }`}
                onClick={() => toggleExpanded(item.id)}
              >
                <div className="flex items-start gap-6">
                  <div className="flex-1 min-w-0">
                    <div className="mb-2 flex flex-wrap gap-2.5 items-center">
                      <span className="mono text-muted text-xs">
                        {item.date}, {item.timestamp}
                      </span>
                      <Badge
                        variant="outline"
                        className="rounded-md bg-surface-active text-muted font-mono text-[11px]"
                      >
                        {item.model}
                      </Badge>
                      {item.duration_sec != null && (
                        <span className="text-[10px] text-muted font-mono">
                          {t("transcriptions.durationSeconds", {
                            seconds: item.duration_sec.toFixed(1),
                          })}
                        </span>
                      )}
                    </div>
                    <div className="text-foreground leading-relaxed text-[13px] break-words select-text pr-12">
                      {item.text}
                    </div>
                  </div>
                  <div className="flex-none flex items-center gap-2 self-start pt-1">
                    <button
                      type="button"
                      aria-expanded={isExpanded}
                      aria-label={t("transcriptions.toggleDetails")}
                      onClick={(e) => {
                        e.stopPropagation();
                        toggleExpanded(item.id);
                      }}
                      className="flex size-8 items-center justify-center rounded-md text-muted hover:text-foreground hover:bg-surface-active transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/60"
                    >
                      <ChevronDown
                        size={18}
                        className={`transition-transform ${isExpanded ? "rotate-180" : ""}`}
                      />
                    </button>
                    <Tooltip>
                      <TooltipTrigger
                        render={
                          <Button
                            variant="ghost"
                            size="icon-sm"
                            onClick={(e) => {
                              e.stopPropagation();
                              deleteItem(item);
                            }}
                            disabled={isDeleting === item.id}
                            className="text-danger hover:text-danger hover:bg-danger/10"
                            aria-label={t("transcriptions.delete")}
                          >
                            {isDeleting === item.id ? (
                              <Loader2 size={14} className="animate-spin" />
                            ) : (
                              <Trash2 size={14} />
                            )}
                          </Button>
                        }
                      />
                      <TooltipContent>{t("transcriptions.delete")}</TooltipContent>
                    </Tooltip>
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={(e) => {
                        e.stopPropagation();
                        handleCopy(item.text);
                      }}
                    >
                      <Copy size={13} />
                      {t("transcriptions.copy")}
                    </Button>
                  </div>
                </div>

                {isExpanded && item.wav_path && (
                  <div className="mt-4 pt-4 border-t border-border/50">
                    <HistoryAudioPlayer
                      id={item.id}
                      path={item.wav_path}
                      durationSec={item.duration_sec}
                    />
                  </div>
                )}
              </div>
            );
          })}
          {hasMore && (
            <div className="p-4 border-t border-border flex justify-center bg-secondary">
              <Button
                variant="outline"
                size="sm"
                onClick={loadMore}
                disabled={loadingMore}
                className="px-8"
              >
                {loadingMore && <Loader2 size={13} className="animate-spin" />}
                {loadingMore
                  ? t("common.loading")
                  : t("transcriptions.loadOlder")}
              </Button>
            </div>
          )}
        </div>
      )}

      <AlertDialog
        open={!!showDeleteModal}
        onOpenChange={(open) => {
          if (!open) setShowDeleteModal(null);
        }}
      >
        <AlertDialogContent size="sm">
          <AlertDialogHeader>
            <AlertDialogTitle>
              {t("transcriptions.deleteConfirmTitle")}
            </AlertDialogTitle>
            <AlertDialogDescription>
              {t("transcriptions.deleteConfirmBody")}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t("common.cancel")}</AlertDialogCancel>
            <AlertDialogAction
              onClick={handleConfirmDelete}
              className="bg-danger text-white hover:bg-danger/90"
            >
              {t("transcriptions.delete")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      <AlertDialog
        open={showConfirmModal}
        onOpenChange={setShowConfirmModal}
      >
        <AlertDialogContent size="sm">
          <AlertDialogHeader>
            <AlertDialogTitle>
              {t("transcriptions.clearConfirmTitle")}
            </AlertDialogTitle>
            <AlertDialogDescription>
              {t("transcriptions.clearConfirmBody")}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t("common.cancel")}</AlertDialogCancel>
            <AlertDialogAction
              onClick={handleConfirmClearHistory}
              className="bg-danger text-white hover:bg-danger/90"
            >
              {t("transcriptions.clearEverything")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
