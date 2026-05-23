import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ChevronDown, Play } from "lucide-react";

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

  const playRecording = (wavPath: string) => {
    invoke("play_wav", { path: wavPath }).catch((err) =>
      console.error("Failed to play audio:", err)
    );
  };

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
  }, []);

  const handleCopy = async (id: string, text: string) => {
    try {
      await navigator.clipboard.writeText(text);
      setCopiedId(id);
      setTimeout(() => {
        setCopiedId(null);
      }, 1500);
    } catch (err) {
      console.error("Failed to copy text:", err);
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
    } catch (err) {
      console.error("Failed to clear history:", err);
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
          <button
            onClick={handleClearHistory}
            className="btn btn-small btn-outline text-red-400 hover:text-red-300 hover:border-red-400/50 shrink-0 transition-all duration-300 cursor-pointer"
          >
            Clear History
          </button>
        )}
      </div>

      {history.length === 0 ? (
        <div className="flex flex-col items-center justify-center p-12 text-center border border-dashed border-border rounded-xl bg-secondary">
          <div className="w-12 h-12 rounded-full bg-surface-active flex items-center justify-center text-muted mb-4 opacity-50">
            "
          </div>
          <h3 className="text-white font-medium mb-1">No transcriptions yet</h3>
          <p className="text-muted text-sm max-w-sm leading-relaxed">
            Record some audio using your global shortcut to start transcribing
            speech to text.
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
                className={`group flex flex-col p-5 transition-all hover:bg-surface-hover border-b border-border last:border-b-0 cursor-pointer ${
                  isExpanded ? "bg-surface-hover" : ""
                }`}
                onClick={() => toggleExpanded(item.id)}
              >
                <div className="flex items-start gap-6">
                  <div className="flex-1 min-w-0">
                    <div className="mb-2 flex flex-wrap gap-2.5 items-center">
                      <span className="mono text-muted-dark text-xs font-mono">
                        {item.date}, {item.timestamp}
                      </span>
                      <span className="inline-flex items-center px-2 py-0.5 rounded text-[11px] font-mono font-medium bg-surface-active text-muted border border-border">
                        {item.model}
                      </span>
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
                    <button
                      onClick={(e) => {
                        e.stopPropagation();
                        deleteItem(item);
                      }}
                      disabled={isDeleting === item.id}
                      className="btn btn-small btn-outline text-red-400 hover:text-red-300 hover:border-red-400/50 w-9 h-9 p-0 flex items-center justify-center transition-all cursor-pointer"
                      title="Delete"
                    >
                      {isDeleting === item.id ? (
                        <span className="w-3 h-3 border-2 border-red-400/40 border-t-red-400 rounded-full animate-spin" />
                      ) : (
                        <svg
                          xmlns="http://www.w3.org/2000/svg"
                          width="14"
                          height="14"
                          viewBox="0 0 24 24"
                          fill="none"
                          stroke="currentColor"
                          strokeWidth="2.25"
                          strokeLinecap="round"
                          strokeLinejoin="round"
                        >
                          <path d="M3 6h18" />
                          <path d="M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6" />
                          <path d="M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2" />
                        </svg>
                      )}
                    </button>
                    <button
                      onClick={(e) => {
                        e.stopPropagation();
                        handleCopy(item.id, item.text);
                      }}
                      className={`btn btn-small w-20 transition-all ${
                        isCopied
                          ? "btn-outline border-emerald-500/30 text-emerald-400"
                          : "btn-outline"
                      }`}
                    >
                      {isCopied ? "Copied!" : "Copy"}
                    </button>
                  </div>
                </div>

                {isExpanded && item.wav_path && (
                  <div className="mt-4 pt-4 border-t border-border/50">
                    <button
                      onClick={(e) => {
                        e.stopPropagation();
                        playRecording(item.wav_path!);
                      }}
                      className="flex items-center gap-3 bg-surface-active hover:bg-surface-hover transition-colors rounded-2xl px-5 py-3 w-full group"
                    >
                      <div className="w-8 h-8 rounded-xl bg-white/10 flex items-center justify-center text-white group-hover:bg-emerald-500/20 transition-colors">
                        <Play size={18} className="ml-0.5" />
                      </div>
                      <div className="flex-1 text-left">
                        <div className="text-sm font-medium text-white">Odtwórz nagranie</div>
                        {item.duration_sec && (
                          <div className="text-xs text-muted font-mono">
                            {item.duration_sec.toFixed(1)}s • Kliknij aby odtworzyć
                          </div>
                        )}
                      </div>
                    </button>
                  </div>
                )}
              </div>
            );
          })}
          {hasMore && (
            <div className="p-4 border-t border-border flex justify-center bg-secondary">
              <button
                onClick={loadMore}
                disabled={loadingMore}
                className="btn btn-small btn-outline px-8 transition-all"
              >
                {loadingMore ? "Loading..." : "Load more older transcriptions"}
              </button>
            </div>
          )}
        </div>
      )}

      {/* MODAL: Confirm Delete Single Item */}
      {showDeleteModal && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm transition-all duration-300">
          <div className="bg-secondary border border-border rounded-xl p-6 max-w-sm w-full mx-4 shadow-2xl animate-in fade-in zoom-in-95 duration-200">
            <h3 className="text-lg font-medium text-white mb-2">
              Delete Transcription?
            </h3>
            <p className="text-muted text-[13px] mb-6 leading-relaxed">
              Are you sure you want to delete this transcription? This action
              cannot be undone.
            </p>
            <div className="flex justify-end gap-3">
              <button
                onClick={() => setShowDeleteModal(null)}
                className="btn btn-outline px-4 py-2 text-xs rounded-md cursor-pointer"
              >
                Cancel
              </button>
              <button
                onClick={handleConfirmDelete}
                className="btn bg-red-600 hover:bg-red-500 text-white border-0 px-4 py-2 text-xs font-semibold rounded-md cursor-pointer"
              >
                Confirm Delete
              </button>
            </div>
          </div>
        </div>
      )}

      {/* MODAL: Confirm Clear All History */}
      {showConfirmModal && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm transition-all duration-300">
          <div className="bg-secondary border border-border rounded-xl p-6 max-w-sm w-full mx-4 shadow-2xl animate-in fade-in zoom-in-95 duration-200">
            <h3 className="text-lg font-medium text-white mb-2">
              Clear Transcription History?
            </h3>
            <p className="text-muted text-[13px] mb-6 leading-relaxed">
              This will permanently delete all local audio recordings and their
              transcribed text from your device. This action cannot be undone.
            </p>
            <div className="flex justify-end gap-3">
              <button
                onClick={() => setShowConfirmModal(false)}
                className="btn btn-outline px-4 py-2 text-xs rounded-md cursor-pointer"
              >
                Cancel
              </button>
              <button
                onClick={handleConfirmClearHistory}
                className="btn bg-red-600 hover:bg-red-500 text-white border-0 px-4 py-2 text-xs font-semibold rounded-md cursor-pointer"
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
