import { cn } from "@/lib/utils";

interface WaveBarProps {
  className?: string;
  /** When true, the bars animate like a live equalizer (used on the splash/HUD). */
  animated?: boolean;
}

/**
 * Simplevoice "WaveBar" symbol — five rounded bars forming a sound wave.
 * Geometry is fixed per the brand book; color is inherited via currentColor.
 */
export function WaveBar({ className, animated = false }: WaveBarProps) {
  return (
    <svg
      viewBox="0 0 364 340"
      className={cn("block", animated && "wavebar-eq", className)}
      aria-label="Simplevoice"
      role="img"
      fill="currentColor"
    >
      <rect x="8" y="98" width="36" height="144" rx="18" />
      <rect x="86" y="50" width="36" height="240" rx="18" />
      <rect x="164" y="8" width="36" height="324" rx="18" />
      <rect x="242" y="50" width="36" height="240" rx="18" />
      <rect x="320" y="98" width="36" height="144" rx="18" />
    </svg>
  );
}
