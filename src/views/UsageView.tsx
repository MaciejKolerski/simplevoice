import { useEffect, useState } from "react";
import { Calendar, Clock, FileText, Cpu, TrendingUp } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

interface ModelStatus {
  active: string | null;
  loading: string | null;
}

interface TranscriptionItem {
  id: string;
  timestamp: string;
  date: string;
  text: string;
  model: string;
  duration_sec?: number;
}

interface ChartBar {
  label: string;
  val: number;
  rawVal: number;
  tooltip: string;
  today?: boolean;
}

export function UsageView() {
  const [activeModel, setActiveModel] = useState<string>("None");
  const [loadingModel, setLoadingModel] = useState<string | null>(null);
  const [isRunningLocally, setIsRunningLocally] = useState<boolean>(true);
  const [history, setHistory] = useState<TranscriptionItem[]>([]);
  const [timeRange, setTimeRange] = useState<"7days" | "30days" | "all">("7days");

  const updateActiveModel = async () => {
    try {
      const engine = localStorage.getItem("asr_engine") || "local";
      if (engine === "openai-cloud") {
        const model = localStorage.getItem("asr_model") || "whisper-1";
        const customModel = localStorage.getItem("asr_custom_model") || "";
        const finalModel = model === "custom" ? customModel : model;
        setActiveModel(finalModel || "None");
        setLoadingModel(null);
        setIsRunningLocally(false);
      } else {
        const status = await invoke<ModelStatus>("get_model_status");
        if (status.loading) {
          const parts = status.loading.split(/[\/\\]/);
          const fname = parts[parts.length - 1];
          setLoadingModel(fname);
          setActiveModel("None");
        } else {
          setLoadingModel(null);
          if (status.active) {
            const parts = status.active.split(/[\/\\]/);
            const fname = parts[parts.length - 1];
            setActiveModel(fname);
          } else {
            setActiveModel("None");
          }
        }
        setIsRunningLocally(true);
      }
    } catch (err) {
      console.error("Failed to query active model status:", err);
      setActiveModel("None");
      setLoadingModel(null);
    }
  };

  const loadHistory = async () => {
    try {
      const raw = await invoke<string>("load_history");
      setHistory(JSON.parse(raw || "[]"));
    } catch (err) {
      console.error("Failed to load transcription history for stats:", err);
    }
  };

  useEffect(() => {
    updateActiveModel();
    loadHistory();

    const handleAsrChanged = () => {
      updateActiveModel();
    };
    const handleTranscriptionAdded = () => {
      loadHistory();
    };

    window.addEventListener("asr-engine-changed", handleAsrChanged);
    window.addEventListener("transcription-added", handleTranscriptionAdded);
    
    let unlistenStatus: (() => void) | null = null;
    listen("model-status-changed", updateActiveModel).then((fn) => {
      unlistenStatus = fn;
    });

    return () => {
      window.removeEventListener("asr-engine-changed", handleAsrChanged);
      window.removeEventListener("transcription-added", handleTranscriptionAdded);
      if (unlistenStatus) unlistenStatus();
    };
  }, []);

  // Helper to parse history timestamp (YYYY-MM-DD_HH-mm-ss) into Date
  const parseIdToDate = (id: string): Date => {
    const match = id.match(/^(\d{4})-(\d{2})-(\d{2})_(\d{2})-(\d{2})-(\d{2})$/);
    if (match) {
      const [_, year, month, day, hour, min, sec] = match;
      return new Date(
        parseInt(year),
        parseInt(month) - 1,
        parseInt(day),
        parseInt(hour),
        parseInt(min),
        parseInt(sec)
      );
    }
    const num = Number(id);
    if (!isNaN(num) && num > 0) {
      return new Date(num);
    }
    return new Date(0);
  };

  const getWordCount = (text: string): number => {
    if (!text) return 0;
    return text.trim().split(/\s+/).filter(w => w.length > 0).length;
  };

  const formatDuration = (seconds: number): string => {
    if (seconds <= 0) return "0s";
    const h = Math.floor(seconds / 3600);
    const m = Math.floor((seconds % 3600) / 60);
    const s = Math.round(seconds % 60);
    if (h > 0) {
      return `${h}h ${m}m`;
    }
    if (m > 0) {
      return `${m}m ${s}s`;
    }
    return `${s}s`;
  };

  // Process history data based on selected time range
  const now = new Date();
  const startOfToday = new Date(now.getFullYear(), now.getMonth(), now.getDate());

  const itemsWithMeta = history.map(item => {
    const parsedDate = parseIdToDate(item.id);
    const duration = item.duration_sec || 0;
    const words = getWordCount(item.text);
    return { ...item, parsedDate, duration, words };
  });

  let totalDuration = 0;
  let totalWords = 0;
  let durationTrend = 0;
  let wordsTrend = 0;
  let bars: ChartBar[] = [];

  const calculateTrend = (current: number, previous: number) => {
    if (previous === 0) {
      return current > 0 ? 100 : 0;
    }
    return Math.round(((current - previous) / previous) * 100);
  };

  if (timeRange === "7days") {
    const currentStart = new Date(startOfToday);
    currentStart.setDate(startOfToday.getDate() - 6);
    
    const prevStart = new Date(currentStart);
    prevStart.setDate(currentStart.getDate() - 7);

    const currentItems = itemsWithMeta.filter(item => item.parsedDate >= currentStart);
    const prevItems = itemsWithMeta.filter(item => item.parsedDate >= prevStart && item.parsedDate < currentStart);

    totalDuration = currentItems.reduce((sum, item) => sum + item.duration, 0);
    totalWords = currentItems.reduce((sum, item) => sum + item.words, 0);

    const prevDuration = prevItems.reduce((sum, item) => sum + item.duration, 0);
    const prevWords = prevItems.reduce((sum, item) => sum + item.words, 0);

    durationTrend = calculateTrend(totalDuration, prevDuration);
    wordsTrend = calculateTrend(totalWords, prevWords);

    // Generate 7 daily bars
    const rawVals: number[] = [];
    for (let i = 6; i >= 0; i--) {
      const d = new Date(startOfToday);
      d.setDate(startOfToday.getDate() - i);
      const dStart = new Date(d.getFullYear(), d.getMonth(), d.getDate());
      const dEnd = new Date(d.getFullYear(), d.getMonth(), d.getDate(), 23, 59, 59, 999);

      const dayItems = itemsWithMeta.filter(item => item.parsedDate >= dStart && item.parsedDate <= dEnd);
      const dayDur = dayItems.reduce((sum, item) => sum + item.duration, 0);
      rawVals.push(dayDur);
    }
    const maxDur = Math.max(...rawVals, 1);

    for (let i = 6; i >= 0; i--) {
      const d = new Date(startOfToday);
      d.setDate(startOfToday.getDate() - i);
      const dStart = new Date(d.getFullYear(), d.getMonth(), d.getDate());
      const dEnd = new Date(d.getFullYear(), d.getMonth(), d.getDate(), 23, 59, 59, 999);

      const dayItems = itemsWithMeta.filter(item => item.parsedDate >= dStart && item.parsedDate <= dEnd);
      const dayDur = dayItems.reduce((sum, item) => sum + item.duration, 0);
      const label = d.toLocaleDateString(undefined, { weekday: "short" });
      const val = dayDur > 0 ? Math.round((dayDur / maxDur) * 90) + 10 : 0;

      bars.push({
        label,
        val,
        rawVal: dayDur,
        tooltip: `${d.toLocaleDateString(undefined, { weekday: 'long', day: 'numeric', month: 'short' })}: ${formatDuration(dayDur)}`,
        today: i === 0
      });
    }

  } else if (timeRange === "30days") {
    const currentStart = new Date(startOfToday);
    currentStart.setDate(startOfToday.getDate() - 29);

    const prevStart = new Date(currentStart);
    prevStart.setDate(currentStart.getDate() - 30);

    const currentItems = itemsWithMeta.filter(item => item.parsedDate >= currentStart);
    const prevItems = itemsWithMeta.filter(item => item.parsedDate >= prevStart && item.parsedDate < currentStart);

    totalDuration = currentItems.reduce((sum, item) => sum + item.duration, 0);
    totalWords = currentItems.reduce((sum, item) => sum + item.words, 0);

    const prevDuration = prevItems.reduce((sum, item) => sum + item.duration, 0);
    const prevWords = prevItems.reduce((sum, item) => sum + item.words, 0);

    durationTrend = calculateTrend(totalDuration, prevDuration);
    wordsTrend = calculateTrend(totalWords, prevWords);

    // Generate 30 daily bars
    const rawVals: number[] = [];
    for (let i = 29; i >= 0; i--) {
      const d = new Date(startOfToday);
      d.setDate(startOfToday.getDate() - i);
      const dStart = new Date(d.getFullYear(), d.getMonth(), d.getDate());
      const dEnd = new Date(d.getFullYear(), d.getMonth(), d.getDate(), 23, 59, 59, 999);

      const dayItems = itemsWithMeta.filter(item => item.parsedDate >= dStart && item.parsedDate <= dEnd);
      const dayDur = dayItems.reduce((sum, item) => sum + item.duration, 0);
      rawVals.push(dayDur);
    }
    const maxDur = Math.max(...rawVals, 1);

    for (let i = 29; i >= 0; i--) {
      const d = new Date(startOfToday);
      d.setDate(startOfToday.getDate() - i);
      const dStart = new Date(d.getFullYear(), d.getMonth(), d.getDate());
      const dEnd = new Date(d.getFullYear(), d.getMonth(), d.getDate(), 23, 59, 59, 999);

      const dayItems = itemsWithMeta.filter(item => item.parsedDate >= dStart && item.parsedDate <= dEnd);
      const dayDur = dayItems.reduce((sum, item) => sum + item.duration, 0);
      
      // Sparse labels to prevent overlap
      let label = "";
      if (i === 29 || i === 20 || i === 10 || i === 0) {
        label = d.toLocaleDateString(undefined, { day: "numeric", month: "short" });
      }

      const val = dayDur > 0 ? Math.round((dayDur / maxDur) * 90) + 10 : 0;

      bars.push({
        label,
        val,
        rawVal: dayDur,
        tooltip: `${d.toLocaleDateString(undefined, { day: 'numeric', month: 'short', year: 'numeric' })}: ${formatDuration(dayDur)}`,
        today: i === 0
      });
    }

  } else {
    // All time
    totalDuration = itemsWithMeta.reduce((sum, item) => sum + item.duration, 0);
    totalWords = itemsWithMeta.reduce((sum, item) => sum + item.words, 0);
    
    // Generate last 6 monthly bars
    const rawVals: number[] = [];
    for (let i = 5; i >= 0; i--) {
      const d = new Date();
      d.setMonth(d.getMonth() - i);
      const mStart = new Date(d.getFullYear(), d.getMonth(), 1);
      const mEnd = new Date(d.getFullYear(), d.getMonth() + 1, 0, 23, 59, 59, 999);

      const mItems = itemsWithMeta.filter(item => item.parsedDate >= mStart && item.parsedDate <= mEnd);
      const mDur = mItems.reduce((sum, item) => sum + item.duration, 0);
      rawVals.push(mDur);
    }
    const maxDur = Math.max(...rawVals, 1);

    for (let i = 5; i >= 0; i--) {
      const d = new Date();
      d.setMonth(d.getMonth() - i);
      const mStart = new Date(d.getFullYear(), d.getMonth(), 1);
      const mEnd = new Date(d.getFullYear(), d.getMonth() + 1, 0, 23, 59, 59, 999);

      const mItems = itemsWithMeta.filter(item => item.parsedDate >= mStart && item.parsedDate <= mEnd);
      const mDur = mItems.reduce((sum, item) => sum + item.duration, 0);
      const label = d.toLocaleDateString(undefined, { month: "short" });
      const val = mDur > 0 ? Math.round((mDur / maxDur) * 90) + 10 : 0;

      bars.push({
        label,
        val,
        rawVal: mDur,
        tooltip: `${d.toLocaleDateString(undefined, { month: 'long', year: 'numeric' })}: ${formatDuration(mDur)}`,
        today: i === 0
      });
    }
  }

  // Dynamic Y-axis calculation
  const chartMaxDur = Math.max(...bars.map(b => b.rawVal), 0);
  const displayMaxDur = chartMaxDur > 0 ? chartMaxDur : 3600;
  const yLabels = [
    formatDuration(displayMaxDur),
    formatDuration(displayMaxDur * 2 / 3),
    formatDuration(displayMaxDur * 1 / 3),
    "0s"
  ];

  const renderTrend = (value: number) => {
    if (timeRange === "all") {
      return (
        <span className="text-muted opacity-70">
          All-time statistics
        </span>
      );
    }
    
    if (value > 0) {
      return (
        <>
          <span className="trend up flex items-center gap-0.5 text-emerald-400 font-semibold whitespace-nowrap shrink-0">
            <TrendingUp size={12} /> +{value}%
          </span>
          <span className="truncate opacity-70">vs last period</span>
        </>
      );
    } else if (value < 0) {
      return (
        <>
          <span className="trend down flex items-center gap-0.5 text-rose-400 font-semibold whitespace-nowrap shrink-0">
            <TrendingUp size={12} className="rotate-180" /> {value}%
          </span>
          <span className="truncate opacity-70">vs last period</span>
        </>
      );
    } else {
      return (
        <>
          <span className="trend flat flex items-center gap-0.5 text-muted font-semibold whitespace-nowrap shrink-0">
            — 0%
          </span>
          <span className="truncate opacity-70">vs last period</span>
        </>
      );
    }
  };

  return (
    <div className="flex flex-col w-full animate-[fadeIn_0.3s_ease-out]">
      <div className="flex flex-col sm:flex-row justify-between items-start sm:items-center gap-4 mb-6 pr-1">
        <h1 className="m-0 text-2xl font-medium text-white tracking-tight">
          Overview
        </h1>
        <div className="flex items-center gap-2 w-full sm:w-auto overflow-hidden">
          <div className="relative flex-1 sm:flex-none sm:min-w-[140px]">
            <select
              value={timeRange}
              onChange={(e) => setTimeRange(e.target.value as any)}
              className="input pl-3 pr-8 py-1.5 w-full text-xs h-9 bg-black border-border rounded-md appearance-none cursor-pointer"
            >
              <option value="7days">Last 7 days</option>
              <option value="30days">Last 30 days</option>
              <option value="all">All time</option>
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
          <div className="stat-value mono truncate">{formatDuration(totalDuration)}</div>
          <div className="muted text-xs mt-3 flex items-center gap-1.5 text-muted-foreground">
            {renderTrend(durationTrend)}
          </div>
        </div>
        <div className="card min-w-0">
          <div className="label-text flex justify-between items-center mb-3">
            <span className="truncate">Words Generated</span>
            <FileText size={14} className="opacity-50 shrink-0" />
          </div>
          <div className="stat-value mono truncate">{totalWords.toLocaleString()}</div>
          <div className="muted text-xs mt-3 flex items-center gap-1.5 text-muted-foreground">
            {renderTrend(wordsTrend)}
          </div>
        </div>
        <div className="card min-w-0 md:col-span-2 lg:col-span-1">
          <div className="label-text flex justify-between items-center mb-3">
            <span className="truncate">Active Model</span>
            <Cpu size={14} className="opacity-50 shrink-0" />
          </div>
          <div className="text-xl leading-tight pt-1 tracking-tight text-white font-medium truncate">
            {loadingModel ? loadingModel : activeModel}
          </div>
          <div className="muted text-xs mt-3 flex items-center gap-1.5 text-muted-foreground">
            <span className={`inline-block w-1.5 h-1.5 rounded-full shrink-0 ${
              loadingModel 
                ? "bg-sky-400 animate-pulse shadow-[0_0_8px_rgba(56,189,248,0.5)]" 
                : activeModel === "None" 
                  ? "bg-amber-400" 
                  : "bg-emerald-400"
            }`}></span>
            <span className="truncate opacity-70">
              {loadingModel 
                ? "Initializing engine..." 
                : activeModel === "None" 
                  ? "No active model" 
                  : isRunningLocally 
                    ? "Running locally" 
                    : "Running in the cloud"}
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
          <div className="flex flex-col justify-between pr-4 pb-6 text-[11px] font-mono text-muted-dark text-right w-16 select-none shrink-0">
            {yLabels.map((lbl, idx) => (
              <span key={idx}>{lbl}</span>
            ))}
          </div>

          <div className="absolute top-1.5 bottom-6 left-16 right-0 flex flex-col justify-between pointer-events-none z-0">
            <div className="h-px w-full border-t border-dashed border-white/10"></div>
            <div className="h-px w-full border-t border-dashed border-white/10"></div>
            <div className="h-px w-full border-t border-dashed border-white/10"></div>
            <div className="h-px w-full"></div>
          </div>

          <div className="flex-1 flex justify-between items-end pb-6 relative z-10 min-w-0 gap-1 sm:gap-2 pl-2">
            {bars.map((day, i) => (
              <div
                key={i}
                className="flex-1 flex flex-col items-center justify-end h-full relative group min-w-0"
              >
                <div className="absolute -top-9 bg-white text-black px-2 py-1 rounded-md text-[10px] sm:text-xs font-mono font-bold opacity-0 group-hover:opacity-100 transition-all duration-200 translate-y-2 group-hover:translate-y-0 pointer-events-none z-20 shadow-lg whitespace-nowrap">
                  {day.tooltip}
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
