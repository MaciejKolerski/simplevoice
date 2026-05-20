import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import Database from "@tauri-apps/plugin-sql";

interface TranscriptionItem {
  id: string;
  timestamp: string;
  date: string;
  text: string;
  model: string;
  wav_path?: string;
}

export function TranscriptionsView() {
  const [history, setHistory] = useState<TranscriptionItem[]>([]);
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const [showConfirmModal, setShowConfirmModal] = useState(false);
  const [isDeleting, setIsDeleting] = useState<string | null>(null);

  const loadHistory = async () => {
    try {
      const db = await Database.load("sqlite:simplevoice.db");
      const result = await db.select<TranscriptionItem[]>(
        "SELECT * FROM transcriptions ORDER BY id DESC",
      );
      setHistory(result);
    } catch (err) {
      console.error("Failed to load transcription history from DB:", err);
    }
  };

  useEffect(() => {
    loadHistory();

    const handleNewTranscription = () => {
      loadHistory();
    };

    window.addEventListener("transcription-added", handleNewTranscription);
    return () => {
      window.removeEventListener("transcription-added", handleNewTranscription);
    };
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
      const db = await Database.load("sqlite:simplevoice.db");
      await db.execute("DELETE FROM transcriptions");
      await db.execute("DELETE FROM daily_usage");

      await invoke("clear_history_cmd");
      setHistory([]);
      window.dispatchEvent(new Event("transcription-added"));
    } catch (err) {
      console.error("Failed to clear history:", err);
    }
  };

  const deleteItem = async (item: TranscriptionItem) => {
    setIsDeleting(item.id);
    try {
      const db = await Database.load("sqlite:simplevoice.db");
      await db.execute("DELETE FROM transcriptions WHERE id = ?", [item.id]);
      if (item.wav_path) {
        await invoke("delete_file_cmd", { path: item.wav_path });
      }
      await loadHistory();
      window.dispatchEvent(new Event("transcription-added"));
    } catch (err) {
      console.error("Failed to delete item:", err);
    } finally {
      setIsDeleting(null);
    }
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
          {history.map((item, idx) => {
            const isCopied = copiedId === item.id;
            return (
              <div
                key={item.id}
                className={`flex items-start p-5 transition-colors hover:bg-surface-hover gap-6 ${
                  idx < history.length - 1 ? "border-b border-border" : ""
                }`}
              >
                <div className="flex-1 min-w-0">
                  <div className="mb-2 flex flex-wrap gap-2.5 items-center">
                    <span className="mono text-muted-dark text-xs font-mono">
                      {item.date}, {item.timestamp}
                    </span>
                    <span className="inline-flex items-center px-2 py-0.5 rounded text-[11px] font-mono font-medium bg-surface-active text-muted border border-border">
                      {item.model}
                    </span>
                  </div>
                  <div className="text-fg leading-relaxed text-[13px] break-words select-text">
                    "{item.text}"
                  </div>
                </div>
                <div className="flex-none flex items-center justify-end pl-4 gap-2">
                  <button
                    onClick={() => deleteItem(item)}
                    disabled={isDeleting === item.id}
                    className="btn btn-small btn-outline text-red-400 hover:text-red-300 hover:border-red-400/50 w-10 px-0 flex justify-center transition-all cursor-pointer"
                    title="Delete item"
                  >
                    {isDeleting === item.id ? (
                      <span className="w-3 h-3 border-2 border-red-400/40 border-t-red-400 rounded-full animate-spin"></span>
                    ) : (
                      <svg
                        xmlns="http://www.w3.org/2000/svg"
                        width="14"
                        height="14"
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        strokeWidth="2"
                        strokeLinecap="round"
                        strokeLinejoin="round"
                      >
                        <path d="M3 6h18"></path>
                        <path d="M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6"></path>
                        <path d="M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2"></path>
                      </svg>
                    )}
                  </button>
                  <button
                    onClick={() => handleCopy(item.id, item.text)}
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
            );
          })}
        </div>
      )}

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
