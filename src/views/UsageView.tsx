import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Calendar, Clock, FileText, Cpu, TrendingUp } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Card } from "@/components/ui/card";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

interface ModelStatus {
  active: string | null;
  loading: string | null;
}

interface DailyUsage {
  date: string;
  words_generated: number;
  time_transcribed_sec: number;
}

interface UsageStats {
  total_transcriptions: number;
  total_words: number;
  total_duration_sec: number;
  daily: DailyUsage[];
}

interface ChartBar {
  label: string;
  val: number;
  rawVal: number;
  tooltip: string;
  today?: boolean;
}

export function UsageView() {
  const { t, i18n } = useTranslation();
  const [activeModel, setActiveModel] = useState<string>("None");
  const [loadingModel, setLoadingModel] = useState<string | null>(null);
  const [isRunningLocally, setIsRunningLocally] = useState<boolean>(true);
  const [totalWordsAllTime, setTotalWordsAllTime] = useState<number>(0);
  const [totalDurationAllTime, setTotalDurationAllTime] = useState<number>(0);
  const [dailyStats, setDailyStats] = useState<DailyUsage[]>([]);
  const [timeRange, setTimeRange] = useState<"7days" | "30days" | "all">(
    "7days",
  );

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

  const loadStats = async () => {
    try {
      const result = await invoke<UsageStats>("get_usage_stats");
      setTotalWordsAllTime(result.total_words);
      setTotalDurationAllTime(result.total_duration_sec);
      setDailyStats(result.daily);
    } catch (err) {
      console.error("Failed to load usage statistics from DB:", err);
    }
  };

  useEffect(() => {
    updateActiveModel();
    loadStats();

    const handleAsrChanged = () => {
      updateActiveModel();
    };
    const handleTranscriptionAdded = () => {
      loadStats();
    };

    window.addEventListener("asr-engine-changed", handleAsrChanged);
    window.addEventListener("transcription-added", handleTranscriptionAdded);

    let unlistenStatus: (() => void) | null = null;
    listen("model-status-changed", updateActiveModel).then((fn) => {
      unlistenStatus = fn;
    });

    return () => {
      window.removeEventListener("asr-engine-changed", handleAsrChanged);
      window.removeEventListener(
        "transcription-added",
        handleTranscriptionAdded,
      );
      if (unlistenStatus) unlistenStatus();
    };
  }, []);

  const numberFormat = new Intl.NumberFormat(i18n.language);

  const formatDuration = (seconds: number): string => {
    if (seconds <= 0) return t("usage.durationSeconds", { value: 0 });
    const h = Math.floor(seconds / 3600);
    const m = Math.floor((seconds % 3600) / 60);
    const s = Math.round(seconds % 60);
    if (h > 0) {
      return t("usage.durationHoursMinutes", { hours: h, minutes: m });
    }
    if (m > 0) {
      return t("usage.durationMinutesSeconds", { minutes: m, seconds: s });
    }
    return t("usage.durationSeconds", { value: s });
  };

  const formatDateString = (d: Date): string => {
    const y = d.getFullYear();
    const m = String(d.getMonth() + 1).padStart(2, "0");
    const r = String(d.getDate()).padStart(2, "0");
    return `${y}-${m}-${r}`;
  };

  const calculateTrend = (current: number, previous: number) => {
    if (previous === 0) {
      return current > 0 ? 100 : 0;
    }
    return Math.round(((current - previous) / previous) * 100);
  };

  const now = new Date();
  const startOfToday = new Date(
    now.getFullYear(),
    now.getMonth(),
    now.getDate(),
  );

  let totalDuration = 0;
  let totalWords = 0;
  let durationTrend = 0;
  let wordsTrend = 0;
  let bars: ChartBar[] = [];

  if (timeRange === "7days") {
    const currentStart = new Date(startOfToday);
    currentStart.setDate(startOfToday.getDate() - 6);

    const prevStart = new Date(currentStart);
    prevStart.setDate(currentStart.getDate() - 7);

    const currentStartStr = formatDateString(currentStart);
    const prevStartStr = formatDateString(prevStart);
    const todayStr = formatDateString(startOfToday);

    const currentItems = dailyStats.filter(
      (item) => item.date >= currentStartStr && item.date <= todayStr,
    );
    const prevItems = dailyStats.filter(
      (item) => item.date >= prevStartStr && item.date < currentStartStr,
    );

    totalDuration = currentItems.reduce(
      (sum, item) => sum + item.time_transcribed_sec,
      0,
    );
    totalWords = currentItems.reduce(
      (sum, item) => sum + item.words_generated,
      0,
    );

    const prevDuration = prevItems.reduce(
      (sum, item) => sum + item.time_transcribed_sec,
      0,
    );
    const prevWords = prevItems.reduce(
      (sum, item) => sum + item.words_generated,
      0,
    );

    durationTrend = calculateTrend(totalDuration, prevDuration);
    wordsTrend = calculateTrend(totalWords, prevWords);

    // Generate 7 daily bars
    const rawVals: number[] = [];
    const dayLabelsAndVals: {
      dateStr: string;
      label: string;
      tooltipLabel: string;
      today: boolean;
    }[] = [];

    for (let i = 6; i >= 0; i--) {
      const d = new Date(startOfToday);
      d.setDate(startOfToday.getDate() - i);
      const dateStr = formatDateString(d);
      const label = d.toLocaleDateString(i18n.language, { weekday: "short" });
      const tooltipLabel = d.toLocaleDateString(i18n.language, {
        weekday: "long",
        day: "numeric",
        month: "short",
      });
      dayLabelsAndVals.push({ dateStr, label, tooltipLabel, today: i === 0 });

      const dayItem = dailyStats.find((item) => item.date === dateStr);
      const dayDur = dayItem ? dayItem.time_transcribed_sec : 0;
      rawVals.push(dayDur);
    }
    const maxDur = Math.max(...rawVals, 1);

    for (let i = 0; i < 7; i++) {
      const info = dayLabelsAndVals[i];
      const dayDur = rawVals[i];
      const val = dayDur > 0 ? Math.round((dayDur / maxDur) * 90) + 10 : 0;

      bars.push({
        label: info.label,
        val,
        rawVal: dayDur,
        tooltip: t("usage.barTooltip", {
          label: info.tooltipLabel,
          duration: formatDuration(dayDur),
        }),
        today: info.today,
      });
    }
  } else if (timeRange === "30days") {
    const currentStart = new Date(startOfToday);
    currentStart.setDate(startOfToday.getDate() - 29);

    const prevStart = new Date(currentStart);
    prevStart.setDate(currentStart.getDate() - 30);

    const currentStartStr = formatDateString(currentStart);
    const prevStartStr = formatDateString(prevStart);
    const todayStr = formatDateString(startOfToday);

    const currentItems = dailyStats.filter(
      (item) => item.date >= currentStartStr && item.date <= todayStr,
    );
    const prevItems = dailyStats.filter(
      (item) => item.date >= prevStartStr && item.date < currentStartStr,
    );

    totalDuration = currentItems.reduce(
      (sum, item) => sum + item.time_transcribed_sec,
      0,
    );
    totalWords = currentItems.reduce(
      (sum, item) => sum + item.words_generated,
      0,
    );

    const prevDuration = prevItems.reduce(
      (sum, item) => sum + item.time_transcribed_sec,
      0,
    );
    const prevWords = prevItems.reduce(
      (sum, item) => sum + item.words_generated,
      0,
    );

    durationTrend = calculateTrend(totalDuration, prevDuration);
    wordsTrend = calculateTrend(totalWords, prevWords);

    // Generate 30 daily bars
    const rawVals: number[] = [];
    const dayLabelsAndVals: {
      dateStr: string;
      label: string;
      tooltipLabel: string;
      today: boolean;
    }[] = [];

    for (let i = 29; i >= 0; i--) {
      const d = new Date(startOfToday);
      d.setDate(startOfToday.getDate() - i);
      const dateStr = formatDateString(d);

      // Sparse labels to prevent overlap
      let label = "";
      if (i === 29 || i === 20 || i === 10 || i === 0) {
        label = d.toLocaleDateString(i18n.language, {
          day: "numeric",
          month: "short",
        });
      }
      const tooltipLabel = d.toLocaleDateString(i18n.language, {
        day: "numeric",
        month: "short",
        year: "numeric",
      });
      dayLabelsAndVals.push({ dateStr, label, tooltipLabel, today: i === 0 });

      const dayItem = dailyStats.find((item) => item.date === dateStr);
      const dayDur = dayItem ? dayItem.time_transcribed_sec : 0;
      rawVals.push(dayDur);
    }
    const maxDur = Math.max(...rawVals, 1);

    for (let i = 0; i < 30; i++) {
      const info = dayLabelsAndVals[i];
      const dayDur = rawVals[i];
      const val = dayDur > 0 ? Math.round((dayDur / maxDur) * 90) + 10 : 0;

      bars.push({
        label: info.label,
        val,
        rawVal: dayDur,
        tooltip: t("usage.barTooltip", {
          label: info.tooltipLabel,
          duration: formatDuration(dayDur),
        }),
        today: info.today,
      });
    }
  } else {
    // All time
    totalDuration = totalDurationAllTime;
    totalWords = totalWordsAllTime;

    // Generate last 6 monthly bars
    const rawVals: number[] = [];
    const monthLabelsAndVals: {
      year: number;
      month: number;
      label: string;
      tooltipLabel: string;
      today: boolean;
    }[] = [];

    for (let i = 5; i >= 0; i--) {
      const d = new Date();
      d.setMonth(d.getMonth() - i);
      const year = d.getFullYear();
      const month = d.getMonth();
      const label = d.toLocaleDateString(i18n.language, { month: "short" });
      const tooltipLabel = d.toLocaleDateString(i18n.language, {
        month: "long",
        year: "numeric",
      });
      monthLabelsAndVals.push({
        year,
        month,
        label,
        tooltipLabel,
        today: i === 0,
      });

      // Sum daily stats for this year & month
      const prefix = `${year}-${String(month + 1).padStart(2, "0")}`;
      const mItems = dailyStats.filter((item) => item.date.startsWith(prefix));
      const mDur = mItems.reduce(
        (sum, item) => sum + item.time_transcribed_sec,
        0,
      );
      rawVals.push(mDur);
    }
    const maxDur = Math.max(...rawVals, 1);

    for (let i = 0; i < 6; i++) {
      const info = monthLabelsAndVals[i];
      const mDur = rawVals[i];
      const val = mDur > 0 ? Math.round((mDur / maxDur) * 90) + 10 : 0;

      bars.push({
        label: info.label,
        val,
        rawVal: mDur,
        tooltip: t("usage.barTooltip", {
          label: info.tooltipLabel,
          duration: formatDuration(mDur),
        }),
        today: info.today,
      });
    }
  }

  // Dynamic Y-axis calculation
  const chartMaxDur = Math.max(...bars.map((b) => b.rawVal), 0);
  const displayMaxDur = chartMaxDur > 0 ? chartMaxDur : 3600;
  const yLabels = [
    formatDuration(displayMaxDur),
    formatDuration((displayMaxDur * 2) / 3),
    formatDuration((displayMaxDur * 1) / 3),
    formatDuration(0),
  ];

  const renderTrend = (value: number) => {
    if (timeRange === "all") {
      return (
        <span className="text-muted opacity-70">
          {t("usage.allTimeStatistics")}
        </span>
      );
    }

    if (value > 0) {
      return (
        <>
          <span className="trend up flex items-center gap-0.5 text-success font-semibold whitespace-nowrap shrink-0">
            <TrendingUp size={12} /> +{numberFormat.format(value)}%
          </span>
          <span className="truncate opacity-70">
            {t("usage.vsLastPeriod")}
          </span>
        </>
      );
    } else if (value < 0) {
      return (
        <>
          <span className="trend down flex items-center gap-0.5 text-danger font-semibold whitespace-nowrap shrink-0">
            <TrendingUp size={12} className="rotate-180" />{" "}
            {numberFormat.format(value)}%
          </span>
          <span className="truncate opacity-70">
            {t("usage.vsLastPeriod")}
          </span>
        </>
      );
    } else {
      return (
        <>
          <span className="trend flat flex items-center gap-0.5 text-muted font-semibold whitespace-nowrap shrink-0">
            — {numberFormat.format(0)}%
          </span>
          <span className="truncate opacity-70">
            {t("usage.vsLastPeriod")}
          </span>
        </>
      );
    }
  };

  const timeRanges: Record<string, string> = {
    "7days": t("usage.last7Days"),
    "30days": t("usage.last30Days"),
    all: t("usage.allTime"),
  };

  return (
    <div className="flex flex-col w-full animate-[fadeIn_0.3s_ease-out]">
      <div className="flex flex-col sm:flex-row justify-between items-start sm:items-center gap-4 mb-6 pr-1">
        <h1 className="m-0 text-2xl font-medium text-white tracking-tight">
          {t("usage.overview")}
        </h1>
        <Select
          value={timeRange}
          onValueChange={(v) => setTimeRange(v as typeof timeRange)}
          items={timeRanges}
        >
          <SelectTrigger className="w-full sm:w-[160px] bg-secondary text-xs">
            <Calendar size={13} className="text-muted" />
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="7days">{t("usage.last7Days")}</SelectItem>
            <SelectItem value="30days">{t("usage.last30Days")}</SelectItem>
            <SelectItem value="all">{t("usage.allTime")}</SelectItem>
          </SelectContent>
        </Select>
      </div>

      {/* Stat Cards - Responsive Grid */}
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4 mb-8">
        <Card className="p-6 gap-0 min-w-0">
          <div className="label-text flex justify-between items-center mb-3">
            <span className="truncate">{t("usage.timeTranscribed")}</span>
            <Clock size={14} className="text-muted-dark shrink-0" />
          </div>
          <div className="stat-value mono truncate">
            {formatDuration(totalDuration)}
          </div>
          <div className="text-xs mt-3 flex items-center gap-1.5 text-muted-foreground">
            {renderTrend(durationTrend)}
          </div>
        </Card>
        <Card className="p-6 gap-0 min-w-0">
          <div className="label-text flex justify-between items-center mb-3">
            <span className="truncate">{t("usage.wordsGenerated")}</span>
            <FileText size={14} className="text-muted-dark shrink-0" />
          </div>
          <div className="stat-value mono truncate">
            {numberFormat.format(totalWords)}
          </div>
          <div className="text-xs mt-3 flex items-center gap-1.5 text-muted-foreground">
            {renderTrend(wordsTrend)}
          </div>
        </Card>
        <Card className="p-6 gap-0 min-w-0 md:col-span-2 lg:col-span-1">
          <div className="label-text flex justify-between items-center mb-3">
            <span className="truncate">{t("usage.activeModel")}</span>
            <Cpu size={14} className="text-muted-dark shrink-0" />
          </div>
          <div className="text-xl leading-tight pt-1 tracking-tight text-white font-medium truncate">
            {loadingModel
              ? loadingModel
              : activeModel === "None"
                ? t("usage.modelNone")
                : activeModel}
          </div>
          <div className="text-xs mt-3 flex items-center gap-1.5 text-muted-foreground">
            <span
              className={`inline-block w-1.5 h-1.5 rounded-full shrink-0 ${
                loadingModel
                  ? "bg-info animate-pulse shadow-[0_0_8px_rgba(96,165,250,0.5)]"
                  : activeModel === "None"
                    ? "bg-warning"
                    : "bg-success"
              }`}
            ></span>
            <span className="truncate opacity-70">
              {loadingModel
                ? t("usage.initializingEngine")
                : activeModel === "None"
                  ? t("usage.noActiveModel")
                  : isRunningLocally
                    ? t("usage.runningLocally")
                    : t("usage.runningInCloud")}
            </span>
          </div>
        </Card>
      </div>

      {/* Activity Details Chart */}
      <div className="bg-secondary border border-border rounded-xl p-6 relative min-h-[340px] lg:min-h-[420px] 2xl:min-h-[500px] flex flex-col w-full overflow-hidden">
        <div className="flex justify-between items-center mb-6">
          <h2 className="m-0 text-base text-white font-medium">
            {t("usage.activityDetails")}
          </h2>
          <div className="hidden sm:flex gap-4">
            <div className="flex items-center gap-2 text-xs text-muted font-medium">
              <div className="w-2 h-2 rounded-full bg-white shadow-[0_0_8px_rgba(255,255,255,0.4)]"></div>
              {t("usage.timeTranscribed")}
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
            <div className="h-px w-full border-t border-dashed border-border"></div>
            <div className="h-px w-full border-t border-dashed border-border"></div>
            <div className="h-px w-full border-t border-dashed border-border"></div>
            <div className="h-px w-full border-t border-border-hover"></div>
          </div>

          <div className="flex-1 flex justify-between items-end pb-6 relative z-10 min-w-0 gap-1.5 sm:gap-2 pl-2">
            {bars.map((day, i) => (
              <div
                key={i}
                className="flex-1 flex flex-col items-center justify-end h-full relative group min-w-0"
              >
                <div className="absolute -top-9 bg-white text-black px-2.5 py-1 rounded-md text-[10px] sm:text-xs font-mono font-bold opacity-0 group-hover:opacity-100 transition-all duration-200 translate-y-2 group-hover:translate-y-0 pointer-events-none z-20 shadow-lg whitespace-nowrap">
                  {day.tooltip}
                  <div className="absolute -bottom-1 left-1/2 -translate-x-1/2 border-l-4 border-r-4 border-l-transparent border-r-transparent border-t-4 border-t-white"></div>
                </div>
                <div
                  className={`w-full max-w-[56px] sm:max-w-[72px] lg:max-w-[100px] xl:max-w-[132px] 2xl:max-w-[176px] h-full rounded-lg flex items-end relative overflow-hidden transition-colors duration-200 ${day.today ? "bg-white/5" : "bg-white/[0.03]"} group-hover:bg-white/10`}
                >
                  <div
                    className={`w-full rounded-lg transition-all duration-300 relative ${day.today ? "bg-gradient-to-t from-white/40 to-white shadow-[0_-4px_24px_rgba(255,255,255,0.18)]" : "bg-gradient-to-t from-white/15 to-white/85 group-hover:to-white"}`}
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
