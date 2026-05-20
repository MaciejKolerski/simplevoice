import { useEffect, useState } from "react";
import { Calendar, Clock, FileText, Cpu, TrendingUp } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";

export function UsageView() {
  const [activeModel, setActiveModel] = useState<string>("None");
  const [isRunningLocally, setIsRunningLocally] = useState<boolean>(true);

  const updateActiveModel = async () => {
    try {
      const engine = localStorage.getItem("asr_engine") || "local";
      if (engine === "openai-cloud") {
        const model = localStorage.getItem("asr_model") || "whisper-1";
        const customModel = localStorage.getItem("asr_custom_model") || "";
        const finalModel = model === "custom" ? customModel : model;
        setActiveModel(finalModel || "None");
        setIsRunningLocally(false);
      } else {
        const activeModelPath = await invoke<string | null>("get_active_model");
        if (activeModelPath) {
          const parts = activeModelPath.split(/[\/\\]/);
          const fname = parts[parts.length - 1];
          setActiveModel(fname);
        } else {
          setActiveModel("None");
        }
        setIsRunningLocally(true);
      }
    } catch (err) {
      console.error("Failed to query active model:", err);
      setActiveModel("None");
    }
  };

  useEffect(() => {
    updateActiveModel();
    window.addEventListener("asr-engine-changed", updateActiveModel);
    return () => {
      window.removeEventListener("asr-engine-changed", updateActiveModel);
    };
  }, []);

  return (
    <div className="flex flex-col w-full">
      <div className="flex flex-col sm:flex-row justify-between items-start sm:items-center gap-4 mb-6 pr-1">
        <h1 className="m-0 text-2xl font-medium text-white tracking-tight">
          Overview
        </h1>
        <div className="flex items-center gap-2 w-full sm:w-auto overflow-hidden">
          <div className="relative flex-1 sm:flex-none sm:min-w-[140px]">
            <select className="input pl-3 pr-8 py-1.5 w-full text-xs h-9 bg-black border-border rounded-md appearance-none cursor-pointer">
              <option>Last 7 days</option>
              <option>Last 30 days</option>
              <option>All time</option>
            </select>
            <div className="absolute right-2 top-1/2 -translate-y-1/2 pointer-events-none text-muted">
              <Calendar size={12} />
            </div>
          </div>
        </div>
      </div>

      {/* Stat Cards - Responsive Grid */}
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4 mb-8">
        <div className="card min-w-0">
          <div className="label-text flex justify-between items-center mb-3">
            <span className="truncate">Time Transcribed</span>
            <Clock size={14} className="opacity-50 shrink-0" />
          </div>
          <div className="stat-value mono truncate">14h 23m</div>
          <div className="muted text-xs mt-3 flex items-center gap-1.5 text-muted-foreground">
            <span className="trend up flex items-center gap-1 text-emerald-400 font-medium whitespace-nowrap shrink-0">
              <TrendingUp size={12} /> 12%
            </span>
            <span className="truncate opacity-70">vs last week</span>
          </div>
        </div>
        <div className="card min-w-0">
          <div className="label-text flex justify-between items-center mb-3">
            <span className="truncate">Words Generated</span>
            <FileText size={14} className="opacity-50 shrink-0" />
          </div>
          <div className="stat-value mono truncate">142,093</div>
          <div className="muted text-xs mt-3 flex items-center gap-1.5 text-muted-foreground">
            <span className="trend up flex items-center gap-1 text-emerald-400 font-medium whitespace-nowrap shrink-0">
              <TrendingUp size={12} /> 5.2%
            </span>
            <span className="truncate opacity-70">vs last week</span>
          </div>
        </div>
        <div className="card min-w-0 md:col-span-2 lg:col-span-1">
          <div className="label-text flex justify-between items-center mb-3">
            <span className="truncate">Active Model</span>
            <Cpu size={14} className="opacity-50 shrink-0" />
          </div>
          <div className="text-xl leading-tight pt-1 tracking-tight text-white font-medium truncate">
            {activeModel}
          </div>
          <div className="muted text-xs mt-3 flex items-center gap-1.5 text-muted-foreground">
            <span className={`inline-block w-1.5 h-1.5 rounded-full shrink-0 ${activeModel === "None" ? "bg-amber-400" : "bg-emerald-400"}`}></span>
            <span className="truncate opacity-70">
              {activeModel === "None" ? "No active model" : isRunningLocally ? "Running locally" : "Running in the cloud"}
            </span>
          </div>
        </div>
      </div>

      {/* Activity Details Chart */}
      <div className="bg-secondary border border-border rounded-xl p-6 relative min-h-[320px] flex flex-col w-full overflow-hidden">
        <div className="flex justify-between items-center mb-6">
          <h2 className="m-0 text-base text-white font-medium">
            Activity Details
          </h2>
          <div className="hidden sm:flex gap-4">
            <div className="flex items-center gap-2 text-xs text-muted font-medium">
              <div className="w-2 h-2 rounded-full bg-white shadow-[0_0_8px_rgba(255,255,255,0.4)]"></div>
              Time Transcribed
            </div>
          </div>
        </div>

        <div className="flex-1 flex relative mt-4 min-w-0">
          <div className="flex flex-col justify-between pr-4 pb-6 text-[11px] font-mono text-muted-dark text-right w-11 select-none shrink-0">
            <span>6h</span>
            <span>4h</span>
            <span>2h</span>
            <span>0h</span>
          </div>

          <div className="absolute top-1.5 bottom-6 left-11 right-0 flex flex-col justify-between pointer-events-none z-0">
            <div className="h-px w-full border-t border-dashed border-white/10"></div>
            <div className="h-px w-full border-t border-dashed border-white/10"></div>
            <div className="h-px w-full border-t border-dashed border-white/10"></div>
            <div className="h-px w-full"></div>
          </div>

          <div className="flex-1 flex justify-between items-end pb-6 relative z-10 min-w-0 gap-1 sm:gap-2">
            {[
              { label: "Mon", val: 41, time: "2h 30m" },
              { label: "Tue", val: 20, time: "1h 15m" },
              { label: "Wed", val: 80, time: "4h 50m" },
              { label: "Thu", val: 62, time: "3h 45m" },
              { label: "Fri", val: 18, time: "1h 05m" },
              { label: "Sat", val: 12, time: "0h 45m" },
              { label: "Sun", val: 90, time: "5h 24m", today: true },
            ].map((day, i) => (
              <div
                key={i}
                className="flex-1 flex flex-col items-center justify-end h-full relative group min-w-0"
              >
                <div className="absolute -top-9 bg-white text-black px-2 py-1 rounded-md text-[10px] sm:text-xs font-mono font-bold opacity-0 group-hover:opacity-100 transition-all duration-200 translate-y-2 group-hover:translate-y-0 pointer-events-none z-20 shadow-lg whitespace-nowrap">
                  {day.time}
                  <div className="absolute -bottom-1 left-1/2 -translate-x-1/2 border-l-4 border-r-4 border-l-transparent border-r-transparent border-t-4 border-t-white"></div>
                </div>
                <div
                  className={`w-full max-w-[48px] h-full rounded-md flex items-end relative overflow-hidden transition-colors duration-200 ${day.today ? "bg-white/5" : "bg-white/2"} group-hover:bg-white/10`}
                >
                  <div
                    className={`w-full rounded-md transition-all duration-300 relative ${day.today ? "bg-gradient-to-t from-white/40 to-white shadow-[0_-4px_20px_rgba(255,255,255,0.15)]" : "bg-gradient-to-t from-white/20 to-white/90"}`}
                    style={{ height: `${day.val}%` }}
                  ></div>
                </div>
                <div
                  className={`absolute -bottom-6 w-full text-center text-[10px] sm:text-xs transition-colors duration-200 ${day.today ? "text-white font-semibold" : "text-muted group-hover:text-white"}`}
                >
                  {day.label}
                </div>
              </div>
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}
