import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ChevronDown, History, Trash2, Copy, Check, Loader2 } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
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
  const [history, setHistory] = useState<TranscriptionItem[]>([]);
  const [offset, setOffset] = useState(0);
  const [hasMore, setHasMore] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const [showConfirmModal, setShowConfirmModal] = useState(false);
  const [showDeleteModal, setShowDeleteModal] =
    useState<TranscriptionItem | null>(null);
  const [isDeleting, setIsDeleting] = useState<string | null>(null);
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const [audioCache, setAudioCache] = useState<Record<string, string>>({});

  useEffect(() => {
    if (expandedId && !audioCache[expandedId]) {
      const item = history.find((h) => h.id === expandedId);
      if (item?.wav_path) {
        invoke<string>("get_audio_base64", { path: item.wav_path })
          .then((base64) => {
            setAudioCache((prev) => ({ ...prev, [expandedId]: base64 }));
          })
          .catch((err) => console.error("Failed to load audio:", err));
      }
    }
  }, [expandedId, history, audioCache]);

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

  const handleCopy = async (id: string, text: string) => {
    try {
      await navigator.clipboard.writeText(text);
      setCopiedId(id);
      toast.success("Copied to clipboard");
      setTimeout(() => {
        setCopiedId(null);
      }, 1500);
    } catch (err) {
      console.error("Failed to copy text:", err);
      toast.error("Couldn't copy to clipboard");
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
      window.dispatchEvent(new Event("transcription-added"));
      toast.success("History cleared");
    } catch (err) {
      console.error("Failed to clear history:", err);
      toast.error("Failed to clear history");
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
      window.dispatchEvent(new Event("transcription-added"));
    } catch (err) {
      console.error("Failed to delete item:", err);
      toast.error("Failed to delete transcription");
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
    <div className="flex flex-col w-full">
      <div className="flex flex-col sm:flex-row justify-between items-start sm:items-end gap-4 mb-6">
        <div>
          <h1 className="m-0 text-2xl font-medium text-white tracking-tight">
            History
          </h1>
          <p className="text-xs text-muted mt-1 leading-normal">
            Your locally recorded and transcribed voice notes.
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
            Clear history
          </Button>
        )}
      </div>

      {history.length === 0 ? (
        <div className="flex flex-col items-center justify-center p-12 text-center border border-dashed border-border rounded-xl bg-secondary">
          <div className="flex size-14 items-center justify-center rounded-full bg-surface-active text-muted mb-4">
            <History size={26} />
          </div>
          <h3 className="text-white font-medium mb-2">No transcriptions yet</h3>
          <p className="text-muted text-sm max-w-md mb-2 leading-relaxed">
            Use your global shortcut to start recording audio. Once finished,
            the recorded speech will be transcribed and stored here in your
            history.
          </p>
        </div>
      ) : (
        <div className="border border-border rounded-xl overflow-hidden bg-secondary">
          {history.map((item) => {
            const isCopied = copiedId === item.id;
            const isExpanded = expandedId === item.id;
            return (
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
                      <span className="mono text-muted-dark text-xs">
                        {item.date}, {item.timestamp}
                      </span>
                      <Badge
                        variant="outline"
                        className="rounded-md bg-surface-active text-muted font-mono text-[11px]"
                      >
                        {item.model}
                      </Badge>
                      {item.duration_sec && (
                        <span className="text-[10px] text-muted font-mono">
                          {item.duration_sec.toFixed(1)}s
                        </span>
                      )}
                    </div>
                    <div className="text-fg leading-relaxed text-[13px] break-words select-text pr-12">
                      "{item.text}"
                    </div>
                  </div>
                  <div className="flex-none flex items-center gap-2 self-start pt-1">
                    <ChevronDown
                      size={18}
                      className={`text-muted transition-transform group-hover:text-fg ${isExpanded ? "rotate-180" : ""}`}
                    />
                    <Button
                      variant="ghost"
                      size="icon-sm"
                      onClick={(e) => {
                        e.stopPropagation();
                        deleteItem(item);
                      }}
                      disabled={isDeleting === item.id}
                      className="text-danger hover:text-danger hover:bg-danger/10"
                      title="Delete"
                    >
                      {isDeleting === item.id ? (
                        <Loader2 size={14} className="animate-spin" />
                      ) : (
                        <Trash2 size={14} />
                      )}
                    </Button>
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={(e) => {
                        e.stopPropagation();
                        handleCopy(item.id, item.text);
                      }}
                      className={`w-[88px] ${
                        isCopied
                          ? "border-success/30 text-success"
                          : ""
                      }`}
                    >
                      {isCopied ? <Check size={13} /> : <Copy size={13} />}
                      {isCopied ? "Copied" : "Copy"}
                    </Button>
                  </div>
                </div>

                {isExpanded && item.wav_path && (
                  <div className="mt-4 pt-4 border-t border-border/50">
                    {audioCache[item.id] ? (
                      <div className="bg-surface-active rounded-2xl p-4">
                        <audio
                          src={`data:audio/wav;base64,${audioCache[item.id]}`}
                          controls
                          className="w-full accent-success"
                          onClick={(e) => e.stopPropagation()}
                        />
                      </div>
                    ) : (
                      <div className="text-muted text-sm py-12 text-center border border-dashed border-border rounded-2xl flex items-center justify-center gap-2">
                        <Loader2 size={14} className="animate-spin" />
                        Loading recording…
                      </div>
                    )}
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
                {loadingMore ? "Loading…" : "Load older transcriptions"}
              </Button>
            </div>
          )}
        </div>
      )}

      {/* Confirm: delete a single item */}
      <AlertDialog
        open={!!showDeleteModal}
        onOpenChange={(open) => {
          if (!open) setShowDeleteModal(null);
        }}
      >
        <AlertDialogContent size="sm">
          <AlertDialogHeader>
            <AlertDialogTitle>Delete transcription?</AlertDialogTitle>
            <AlertDialogDescription>
              This permanently removes this transcription and its audio
              recording from your device. This can't be undone.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              onClick={handleConfirmDelete}
              className="bg-danger text-white hover:bg-danger/90"
            >
              Delete
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      {/* Confirm: clear all history */}
      <AlertDialog
        open={showConfirmModal}
        onOpenChange={setShowConfirmModal}
      >
        <AlertDialogContent size="sm">
          <AlertDialogHeader>
            <AlertDialogTitle>Clear all history?</AlertDialogTitle>
            <AlertDialogDescription>
              This permanently deletes every local audio recording and its
              transcribed text from your device. This can't be undone.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              onClick={handleConfirmClearHistory}
              className="bg-danger text-white hover:bg-danger/90"
            >
              Clear everything
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
