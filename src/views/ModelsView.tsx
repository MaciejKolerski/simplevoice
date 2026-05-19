export function ModelsView() {
  return (
    <div className="flex flex-col w-full">
      <div className="flex flex-col sm:flex-row justify-between items-start sm:items-end gap-4 mb-6">
        <h1 className="m-0 text-2xl font-medium text-white tracking-tight">
          Local Models
        </h1>
        <button className="btn btn-outline btn-small shrink-0">
          Scan Directory
        </button>
      </div>

      <div className="border border-border rounded-xl overflow-hidden bg-secondary">
        {/* Model Item 1 */}
        <div className="flex flex-col lg:flex-row items-start lg:items-center p-5 border-b border-border transition-colors hover:bg-surface-hover gap-6">
          <div className="flex-1 min-w-0">
            <div className="flex items-center gap-3 mb-1">
              <h3 className="m-0 font-medium text-white truncate">
                Whisper v3 Large
              </h3>
              <span className="inline-flex items-center px-2 py-0.5 rounded text-[11px] font-mono font-medium text-emerald-400 bg-emerald-400/5 border border-emerald-400/20 shrink-0">
                Active
              </span>
            </div>
            <div className="text-muted text-[13px] truncate">
              High precision, multi-lingual. Best for final passes.
            </div>
          </div>

          <div className="flex flex-col sm:flex-row items-start sm:items-center gap-6 w-full lg:w-auto">
            <div className="flex flex-col gap-2 w-full sm:w-[180px]">
              <div className="text-[11px] text-muted-dark font-semibold uppercase tracking-wider flex justify-between items-center">
                <span>Quality</span>
                <span className="text-white font-mono">95%</span>
              </div>
              <div className="w-full h-1 bg-surface-active rounded-full overflow-hidden">
                <div
                  className="h-full bg-white rounded-full"
                  style={{ width: "95%" }}
                ></div>
              </div>
            </div>

            <div className="flex flex-col gap-2 w-full sm:w-[180px]">
              <div className="text-[11px] text-muted-dark font-semibold uppercase tracking-wider flex justify-between items-center">
                <span>Speed</span>
                <span className="text-white font-mono">40%</span>
              </div>
              <div className="w-full h-1 bg-surface-active rounded-full overflow-hidden">
                <div
                  className="h-full bg-muted rounded-full"
                  style={{ width: "40%" }}
                ></div>
              </div>
            </div>

            <div className="flex items-center justify-between lg:justify-end gap-6 w-full lg:w-[140px] shrink-0">
              <div className="mono text-muted text-xs font-mono whitespace-nowrap">
                2.9 GB
              </div>
              <button className="btn btn-outline btn-small lg:hidden">
                Load
              </button>
            </div>
          </div>
        </div>

        {/* Model Item 2 */}
        <div className="flex flex-col lg:flex-row items-start lg:items-center p-5 transition-colors hover:bg-surface-hover gap-6">
          <div className="flex-1 min-w-0">
            <div className="flex items-center gap-3 mb-1">
              <h3 className="m-0 font-medium text-white truncate">
                Distil-Whisper
              </h3>
            </div>
            <div className="text-muted text-[13px] truncate">
              Optimized for speed. Good for real-time dictation.
            </div>
          </div>

          <div className="flex flex-col sm:flex-row items-start sm:items-center gap-6 w-full lg:w-auto">
            <div className="flex flex-col gap-2 w-full sm:w-[180px]">
              <div className="text-[11px] text-muted-dark font-semibold uppercase tracking-wider flex justify-between items-center">
                <span>Quality</span>
                <span className="text-white font-mono">75%</span>
              </div>
              <div className="w-full h-1 bg-surface-active rounded-full overflow-hidden">
                <div
                  className="h-full bg-muted rounded-full"
                  style={{ width: "75%" }}
                ></div>
              </div>
            </div>

            <div className="flex flex-col gap-2 w-full sm:w-[180px]">
              <div className="text-[11px] text-muted-dark font-semibold uppercase tracking-wider flex justify-between items-center">
                <span>Speed</span>
                <span className="text-white font-mono">90%</span>
              </div>
              <div className="w-full h-1 bg-surface-active rounded-full overflow-hidden">
                <div
                  className="h-full bg-white rounded-full"
                  style={{ width: "90%" }}
                ></div>
              </div>
            </div>

            <div className="flex items-center justify-between lg:justify-end gap-6 w-full lg:w-[140px] shrink-0">
              <div className="mono text-muted text-xs font-mono whitespace-nowrap">
                480 MB
              </div>
              <button className="btn btn-outline btn-small">Load</button>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
