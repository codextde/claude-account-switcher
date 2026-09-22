import type { ButtonHTMLAttributes, ReactNode } from "react";
import { motion } from "motion/react";

export function IconButton({
  label,
  children,
  className = "",
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & { label: string; children: ReactNode }) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      className={`focus-ring inline-flex h-8 w-8 items-center justify-center rounded-full ink-2 transition-colors hover:panel-hover hover:ink disabled:opacity-40 disabled:hover:bg-transparent ${className}`}
      {...rest}
    >
      {children}
    </button>
  );
}

export function Button({
  variant = "primary",
  className = "",
  children,
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & { variant?: "primary" | "ghost" | "danger" }) {
  const base =
    "focus-ring inline-flex items-center justify-center gap-1.5 rounded-full px-3.5 py-1.5 text-[13px] font-medium transition-all disabled:opacity-40 disabled:pointer-events-none active:scale-[0.98]";
  const variants = {
    primary:
      "bg-gradient-to-br from-accent-soft to-accent text-white shadow-[0_6px_18px_-8px_rgba(217,119,87,0.9)] hover:brightness-110",
    ghost: "panel ink-2 hover:panel-hover hover:ink",
    danger: "bg-critical/15 text-critical hover:bg-critical/25",
  };
  return (
    <button type="button" className={`${base} ${variants[variant]} ${className}`} {...rest}>
      {children}
    </button>
  );
}

export function Badge({ children, tone = "neutral" }: { children: ReactNode; tone?: "neutral" | "accent" | "ok" | "warn" | "critical" }) {
  const tones = {
    neutral: "panel ink-2",
    accent: "bg-accent/15 text-accent",
    ok: "bg-ok/15 text-ok",
    warn: "bg-warn/15 text-warn",
    critical: "bg-critical/15 text-critical",
  };
  return (
    <span className={`inline-flex items-center rounded-full px-2 py-0.5 text-[11px] font-semibold tracking-wide ${tones[tone]}`}>
      {children}
    </span>
  );
}

export function Toggle({ checked, onChange, label }: { checked: boolean; onChange: (v: boolean) => void; label: string }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      onClick={() => onChange(!checked)}
      className={`focus-ring relative h-6 w-10 shrink-0 rounded-full transition-colors ${checked ? "bg-accent" : "track"}`}
    >
      <motion.span
        layout
        transition={{ type: "spring", stiffness: 600, damping: 32 }}
        className="absolute top-0.5 h-5 w-5 rounded-full bg-white shadow"
        style={{ left: checked ? 18 : 2 }}
      />
    </button>
  );
}

export function Segmented<T extends string>({
  value,
  options,
  onChange,
}: {
  value: T;
  options: { value: T; label: string }[];
  onChange: (v: T) => void;
}) {
  return (
    <div className="panel inline-flex rounded-full p-0.5">
      {options.map((o) => (
        <button
          key={o.value}
          type="button"
          onClick={() => onChange(o.value)}
          className={`focus-ring relative rounded-full px-3 py-1 text-[12px] font-medium transition-colors ${
            o.value === value ? "ink" : "ink-3 hover:ink-2"
          }`}
        >
          {o.value === value && (
            <motion.span
              layoutId="segmented-pill"
              className="absolute inset-0 rounded-full bg-[rgb(var(--panel)/0.12)]"
              transition={{ type: "spring", stiffness: 500, damping: 35 }}
            />
          )}
          <span className="relative">{o.label}</span>
        </button>
      ))}
    </div>
  );
}

export function Spinner({ className = "" }: { className?: string }) {
  return (
    <span
      className={`spin inline-block h-4 w-4 rounded-full border-2 border-[rgb(var(--ink-3))] border-t-accent ${className}`}
      aria-hidden
    />
  );
}
