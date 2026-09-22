import type { AccountView, ModelWindow, Usage, UsageWindow } from "./types";

export const FIVE_HOURS_MS = 5 * 60 * 60 * 1000;
export const SEVEN_DAYS_MS = 7 * 24 * 60 * 60 * 1000;

export function pct(window: UsageWindow | null | undefined): number | null {
  const v = window?.utilization;
  return typeof v === "number" && Number.isFinite(v) ? Math.max(0, Math.min(100, v)) : null;
}

/** The higher of the two watched windows: what auto-switch acts on. */
export function bindingPct(usage: Usage | null | undefined): number | null {
  const a = pct(usage?.fiveHour);
  const b = pct(usage?.sevenDay);
  if (a === null) return b;
  if (b === null) return a;
  return Math.max(a, b);
}

export interface LimitRow {
  /** Short label for tight rows: "5h", "wk", "Fable". */
  short: string;
  /** Full label: "Session", "Weekly", "Fable". */
  label: string;
  hint: string;
  window: UsageWindow | null;
  /** Length of the window, for the elapsed-time marker. */
  span: number;
}

/**
 * Every limit worth showing for an account, in display order:
 * the 5-hour and 7-day windows first, then each per-model weekly window.
 * Model windows without a reading are dropped; the two base windows always appear.
 */
export function limitRows(usage: Usage | null | undefined, models: ModelWindow[] | undefined): LimitRow[] {
  const rows: LimitRow[] = [
    { short: "5h", label: "Session", hint: "5-hour window", window: usage?.fiveHour ?? null, span: FIVE_HOURS_MS },
    { short: "wk", label: "Weekly", hint: "7-day window", window: usage?.sevenDay ?? null, span: SEVEN_DAYS_MS },
  ];
  for (const m of models ?? []) {
    if (pct(m) === null) continue;
    rows.push({ short: m.label, label: m.label, hint: "7-day model window", window: m, span: SEVEN_DAYS_MS });
  }
  return rows;
}

export function resetsAtMs(window: UsageWindow | null | undefined): number | null {
  if (!window?.resetsAt) return null;
  const t = Date.parse(window.resetsAt);
  return Number.isFinite(t) ? t : null;
}

/** Compact countdown: "now", "12m", "2h 14m", "3d 6h". */
export function countdown(targetMs: number | null, now = Date.now()): string | null {
  if (targetMs === null) return null;
  const remaining = Math.floor((targetMs - now) / 1000);
  if (remaining <= 0) return "now";
  const d = Math.floor(remaining / 86_400);
  const h = Math.floor((remaining % 86_400) / 3600);
  const m = Math.floor((remaining % 3600) / 60);
  if (d > 0) return `${d}d ${h}h`;
  if (h > 0) return `${h}h ${m}m`;
  return `${Math.max(m, 1)}m`;
}

/** Fraction of the window already elapsed, 0-100. */
export function elapsedPct(window: UsageWindow | null | undefined, windowMs: number, now = Date.now()): number | null {
  const reset = resetsAtMs(window);
  if (reset === null) return null;
  const elapsed = 1 - (reset - now) / windowMs;
  return Math.max(0, Math.min(1, elapsed)) * 100;
}

export function relativeTime(ms: number | null | undefined, now = Date.now()): string {
  if (!ms) return "never";
  const s = Math.max(0, Math.floor((now - ms) / 1000));
  if (s < 10) return "just now";
  if (s < 60) return `${s}s ago`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ago`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h ago`;
  return `${Math.floor(h / 24)}d ago`;
}

export type Tone = "ok" | "warn" | "critical";

export function tone(value: number | null, threshold: number): Tone {
  if (value === null) return "ok";
  if (value >= threshold) return "critical";
  if (value >= threshold - 20) return "warn";
  return "ok";
}

export function planLabel(account: AccountView): string {
  const sub = account.subscriptionType?.toLowerCase();
  const tier = account.rateLimitTier ?? "";
  if (sub === "max") {
    const m = tier.match(/max_(\d+)x/);
    return m ? `Max ${m[1]}×` : "Max";
  }
  if (sub === "pro") return "Pro";
  if (sub === "team") return "Team";
  if (sub === "enterprise") return "Enterprise";
  if (sub) return sub[0].toUpperCase() + sub.slice(1);
  return "";
}

export function initials(account: AccountView): string {
  const source = account.displayName?.trim() || account.email;
  const parts = source.split(/[\s@._-]+/).filter(Boolean);
  const first = parts[0]?.[0] ?? "?";
  const second = parts.length > 1 ? parts[1][0] : "";
  return (first + second).toUpperCase();
}
