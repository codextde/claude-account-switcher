import { motion } from "motion/react";
import type { Tone } from "../lib/format";

const fills: Record<Tone, string> = {
  ok: "linear-gradient(90deg, #34d399, #6ee7b7)",
  warn: "linear-gradient(90deg, #f59e0b, #fbbf24)",
  critical: "linear-gradient(90deg, #ef4444, #f87171)",
};

/**
 * Animated horizontal progress bar with an optional marker showing how much of the
 * rate-limit window has elapsed. Bar ahead of the marker means quota burns faster
 * than the clock.
 */
export default function UsageBar({
  value,
  tone,
  elapsed,
  size = "md",
  loading = false,
}: {
  value: number | null;
  tone: Tone;
  elapsed?: number | null;
  size?: "sm" | "md" | "lg";
  loading?: boolean;
}) {
  const height = size === "lg" ? "h-2.5" : size === "sm" ? "h-1" : "h-1.5";
  if (loading) return <div className={`shimmer w-full rounded-full ${height}`} />;
  return (
    <div className={`track relative w-full overflow-hidden rounded-full ${height}`} role="progressbar" aria-valuenow={value ?? undefined}>
      <motion.div
        className="absolute inset-y-0 left-0 rounded-full"
        initial={false}
        animate={{ width: `${value ?? 0}%` }}
        transition={{ type: "spring", stiffness: 120, damping: 24, mass: 0.6 }}
        style={{ background: fills[tone], minWidth: value && value > 0 ? 4 : 0 }}
      />
      {typeof elapsed === "number" && size !== "sm" && (
        <div
          className="absolute inset-y-0 w-px bg-[rgb(var(--ink)/0.35)]"
          style={{ left: `${elapsed}%` }}
          title="Time elapsed in this window"
        />
      )}
    </div>
  );
}
