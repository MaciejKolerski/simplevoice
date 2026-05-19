export function TranscriptionsView() {
  return (
    <div className="flex flex-col">
      <h1 className="mb-6 text-2xl font-medium text-white tracking-tight">
        History
      </h1>

      <div className="border border-border rounded-xl overflow-hidden bg-secondary">
        <div className="flex items-start p-5 border-b border-border transition-colors hover:bg-surface-hover">
          <div className="flex-1">
            <div className="mb-2 flex gap-3 items-center">
              <span className="mono text-muted-dark text-xs font-mono">
                Today, 10:42 AM
              </span>
              <span className="inline-flex items-center px-2 py-0.5 rounded text-[11px] font-mono font-medium bg-surface-active text-muted border border-border">
                Whisper v3 Large
              </span>
              <span className="text-muted text-xs">45m 12s</span>
            </div>
            <div className="text-fg leading-relaxed text-[13px]">
              "Alright, let's break down the new architecture. The main concept
              here is that we're completely decoupling the frontend client from
              the state management layer. This means we can swap out..."
            </div>
          </div>
          <div className="flex-none text-right pl-4">
            <button className="btn btn-outline btn-small">Copy</button>
          </div>
        </div>

        <div className="flex items-start p-5 transition-colors hover:bg-surface-hover">
          <div className="flex-1">
            <div className="mb-2 flex gap-3 items-center">
              <span className="mono text-muted-dark text-xs font-mono">
                Yesterday, 2:15 PM
              </span>
              <span className="inline-flex items-center px-2 py-0.5 rounded text-[11px] font-mono font-medium bg-surface-active text-muted border border-border">
                Distil-Whisper
              </span>
              <span className="text-muted text-xs">1h 05m</span>
            </div>
            <div className="text-fg leading-relaxed text-[13px]">
              "Initial thoughts on the refactor: it's looking much cleaner. I
              noticed a few edge cases in the data fetching hooks that we need
              to address before merging."
            </div>
          </div>
          <div className="flex-none text-right pl-4">
            <button className="btn btn-outline btn-small">Copy</button>
          </div>
        </div>
      </div>
    </div>
  );
}
