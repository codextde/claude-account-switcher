import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  AlertTriangle,
  ArrowRightLeft,
  Check,
  ExternalLink,
  KeyRound,
  Plus,
  Power,
  RefreshCw,
  Settings as SettingsIcon,
  Trash2,
  X,
  Zap,
} from "lucide-react";
import { api, errorMessage, useTick } from "../lib/api";
import {
  FIVE_HOURS_MS,
  SEVEN_DAYS_MS,
  bindingPct,
  countdown,
  elapsedPct,
  initials,
  pct,
  planLabel,
  relativeTime,
  resetsAtMs,
  tone,
} from "../lib/format";
import type { AccountView, Snapshot } from "../lib/types";
import UsageBar from "./UsageBar";
import { Badge, Button, IconButton, Spinner } from "./ui";

export default function Popover({ snapshot }: { snapshot: Snapshot | null }) {
  const now = useTick(15_000);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  useEffect(() => {
    if (!error) return;
    const id = window.setTimeout(() => setError(null), 8000);
    return () => window.clearTimeout(id);
  }, [error]);

  const run = async (key: string, fn: () => Promise<unknown>) => {
    setBusy(key);
    setError(null);
    try {
      await fn();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(null);
    }
  };

  if (!snapshot) {
    return (
      <Shell>
        <div className="flex h-40 items-center justify-center">
          <Spinner />
        </div>
      </Shell>
    );
  }

  const active = snapshot.accounts.find((a) => a.isActive) ?? null;
  const others = snapshot.accounts.filter((a) => !a.isActive);
  const threshold = snapshot.settings.threshold;

  return (
    <Shell>
      {/* Header */}
      <header className="flex items-center gap-3 px-5 pt-4 pb-3">
        <Avatar account={active} />
        <div className="min-w-0 flex-1">
          <div className="truncate text-[14px] font-semibold leading-tight">
            {active ? active.displayName || active.email : snapshot.unknownActiveEmail ? "Unknown account" : "No active account"}
          </div>
          <div className="truncate text-[12px] ink-3">
            {active ? active.email : snapshot.unknownActiveEmail ?? "Add an account to get started"}
          </div>
        </div>
        {active && planLabel(active) && <Badge tone="accent">{planLabel(active)}</Badge>}
        <IconButton
          label="Refresh usage"
          disabled={snapshot.refreshing}
          onClick={() => run("refresh", api.refreshUsage)}
        >
          <RefreshCw size={15} className={snapshot.refreshing ? "spin" : ""} />
        </IconButton>
        <IconButton label="Settings" onClick={() => api.openSettings()}>
          <SettingsIcon size={16} />
        </IconButton>
      </header>

      <div className="min-h-0 flex-1 overflow-y-auto px-5 pb-3">
        {/* Alerts */}
        <AnimatePresence initial={false}>
          {!snapshot.cli.available && (
            <Notice key="cli" tone="warn" icon={<AlertTriangle size={14} />}>
              Claude Code CLI not found. Install it or set its path in Settings.
            </Notice>
          )}
          {snapshot.unknownActiveEmail && (
            <Notice key="unknown" tone="warn" icon={<KeyRound size={14} />}>
              The CLI is logged in as {snapshot.unknownActiveEmail}, which is not saved yet.
              <button className="ml-1 font-semibold underline underline-offset-2" onClick={() => run("adopt", api.adoptCurrent)}>
                Save it
              </button>
            </Notice>
          )}
          {error && (
            <Notice key="error" tone="critical" icon={<AlertTriangle size={14} />} onClose={() => setError(null)}>
              {error}
            </Notice>
          )}
        </AnimatePresence>

        {/* Active usage */}
        {active ? (
          <ActiveUsage account={active} threshold={threshold} now={now} refreshing={snapshot.refreshing} />
        ) : (
          <EmptyState cliAvailable={snapshot.cli.available} busy={busy === "add"} onAdd={() => run("add", api.addAccount)} />
        )}

        {/* Other accounts */}
        {snapshot.accounts.length > 0 && (
          <section className="mt-4">
            <div className="mb-2 flex items-center justify-between">
              <h2 className="text-[11px] font-semibold uppercase tracking-[0.12em] ink-3">Accounts</h2>
              <button
                type="button"
                disabled={!snapshot.cli.available || snapshot.login.inProgress}
                onClick={() => run("add", api.addAccount)}
                className="focus-ring inline-flex items-center gap-1 rounded-full px-2 py-1 text-[12px] font-medium text-accent transition-colors hover:bg-accent/10 disabled:opacity-40"
              >
                <Plus size={13} /> Add
              </button>
            </div>
            <ul className="flex flex-col gap-1.5">
              <AnimatePresence initial={false}>
                {[...(active ? [active] : []), ...others].map((a) => (
                  <AccountRow
                    key={a.id}
                    account={a}
                    threshold={threshold}
                    busy={busy}
                    onSwitch={() => run(`switch:${a.id}`, () => api.switchAccount(a.id))}
                    onReauth={() => run(`reauth:${a.id}`, () => api.reauthenticate(a.id))}
                    onRemove={() => run(`remove:${a.id}`, () => api.removeAccount(a.id))}
                  />
                ))}
              </AnimatePresence>
            </ul>
          </section>
        )}
      </div>

      {/* Footer */}
      <footer className="flex items-center gap-2 border-t hairline px-5 py-2.5 text-[11px] ink-3">
        <Zap size={12} className={snapshot.settings.autoSwitch ? "text-accent" : ""} />
        <span className="truncate">
          {snapshot.settings.autoSwitch ? `Auto-switch at ${threshold}%` : "Auto-switch off"}
          {snapshot.lastEvent ? ` · ${snapshot.lastEvent.message}` : snapshot.lastRefreshAt ? ` · updated ${relativeTime(snapshot.lastRefreshAt, now)}` : ""}
        </span>
        <span className="flex-1" />
        <IconButton label="Quit" className="h-7 w-7" onClick={() => api.quit()}>
          <Power size={13} />
        </IconButton>
      </footer>

      {/* Login overlay */}
      <AnimatePresence>
        {snapshot.login.inProgress && (
          <motion.div
            key="login"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            className="absolute inset-0 z-20 flex items-center justify-center bg-[rgb(var(--surface)/0.6)] p-6 backdrop-blur-md"
          >
            <motion.div
              initial={{ scale: 0.96, y: 8 }}
              animate={{ scale: 1, y: 0 }}
              exit={{ scale: 0.96, y: 8 }}
              className="panel w-full rounded-xl2 p-5 text-center shadow-2xl"
            >
              <div className="mx-auto mb-3 flex h-12 w-12 items-center justify-center rounded-full bg-accent/15 text-accent">
                <Spinner className="h-5 w-5" />
              </div>
              <div className="text-[14px] font-semibold">Waiting for browser login</div>
              <p className="mt-1 text-[12px] ink-2">
                Finish signing in to Claude in your browser. Your other accounts stay saved.
              </p>
              <div className="mt-4 flex justify-center gap-2">
                {snapshot.login.url && (
                  <Button variant="ghost" onClick={() => openUrl(snapshot.login.url!)}>
                    <ExternalLink size={13} /> Open login page
                  </Button>
                )}
                <Button variant="danger" onClick={() => api.cancelLogin()}>
                  Cancel
                </Button>
              </div>
            </motion.div>
          </motion.div>
        )}
      </AnimatePresence>
    </Shell>
  );
}

/** Popover chrome. Reports its natural height so the window shrinks and grows with the content. */
function Shell({ children }: { children: React.ReactNode }) {
  const ref = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    let last = 0;
    const report = () => {
      const h = Math.ceil(el.getBoundingClientRect().height);
      if (h > 0 && h !== last) {
        last = h;
        api.resizePopover(h).catch(() => undefined);
      }
    };
    report();
    const ro = new ResizeObserver(report);
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  return (
    <div
      ref={ref}
      className="relative flex max-h-[720px] min-h-[160px] w-full flex-col overflow-hidden surface rounded-2xl border hairline shadow-[0_24px_60px_-20px_rgba(0,0,0,0.6)]"
    >
      {children}
    </div>
  );
}

function Avatar({ account, size = 36 }: { account: AccountView | null; size?: number }) {
  return (
    <div
      className="flex shrink-0 items-center justify-center rounded-full bg-gradient-to-br from-accent-soft to-accent text-[13px] font-bold text-white shadow-[inset_0_-2px_6px_rgba(0,0,0,0.25)]"
      style={{ width: size, height: size, fontSize: size * 0.36 }}
      aria-hidden
    >
      {account ? initials(account) : "?"}
    </div>
  );
}

function Notice({
  tone: t,
  icon,
  children,
  onClose,
}: {
  tone: "warn" | "critical";
  icon: React.ReactNode;
  children: React.ReactNode;
  onClose?: () => void;
}) {
  const cls = t === "warn" ? "bg-warn/12 text-warn" : "bg-critical/12 text-critical";
  return (
    <motion.div
      layout
      initial={{ opacity: 0, y: -6 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: -6 }}
      className={`mb-3 flex items-start gap-2 rounded-xl px-3 py-2 text-[12px] leading-snug ${cls}`}
    >
      <span className="mt-0.5 shrink-0">{icon}</span>
      <span className="flex-1">{children}</span>
      {onClose && (
        <button type="button" onClick={onClose} className="shrink-0 opacity-70 hover:opacity-100" aria-label="Dismiss">
          <X size={13} />
        </button>
      )}
    </motion.div>
  );
}

function ActiveUsage({
  account,
  threshold,
  now,
  refreshing,
}: {
  account: AccountView;
  threshold: number;
  now: number;
  refreshing: boolean;
}) {
  const usage = account.usage;
  const loading = !usage && refreshing;
  const rows = [
    { label: "Session", hint: "5-hour window", window: usage?.fiveHour ?? null, span: FIVE_HOURS_MS },
    { label: "Weekly", hint: "7-day window", window: usage?.sevenDay ?? null, span: SEVEN_DAYS_MS },
  ];
  const models = [
    { label: "Opus 7d", window: usage?.sevenDayOpus ?? null },
    { label: "Sonnet 7d", window: usage?.sevenDaySonnet ?? null },
  ].filter((m) => pct(m.window) !== null);

  return (
    <section className="panel rounded-xl2 px-4 pt-3 pb-3.5">
      {rows.map((r) => {
        const v = pct(r.window);
        const t = tone(v, threshold);
        const reset = countdown(resetsAtMs(r.window), now);
        return (
          <div key={r.label} className="py-2 first:pt-0 last:pb-0">
            <div className="mb-1.5 flex items-baseline justify-between">
              <div className="flex items-baseline gap-1.5">
                <span className="text-[13px] font-medium">{r.label}</span>
                <span className="text-[11px] ink-3">{r.hint}</span>
              </div>
              <div className="flex items-baseline gap-2 tnum">
                {reset && <span className="text-[11px] ink-3">resets in {reset}</span>}
                <motion.span
                  key={v ?? "na"}
                  initial={{ opacity: 0.4 }}
                  animate={{ opacity: 1 }}
                  className={`text-[20px] font-semibold leading-none tracking-tight ${
                    t === "critical" ? "text-critical" : t === "warn" ? "text-warn" : ""
                  }`}
                >
                  {v === null ? (loading ? "…" : "–") : `${Math.round(v)}%`}
                </motion.span>
              </div>
            </div>
            <UsageBar value={v} tone={t} elapsed={elapsedPct(r.window, r.span, now)} size="lg" loading={loading} />
          </div>
        );
      })}

      {(models.length > 0 || account.usageError) && (
        <div className="mt-2.5 flex flex-wrap items-center gap-1.5 border-t hairline pt-2.5">
          {models.map((m) => {
            const v = pct(m.window)!;
            return (
              <span key={m.label} className="inline-flex items-center gap-1.5 rounded-full track px-2 py-0.5 text-[11px] ink-2 tnum">
                {m.label}
                <span className="font-semibold ink">{Math.round(v)}%</span>
              </span>
            );
          })}
          {account.usageError && (
            <span className="inline-flex items-center gap-1 text-[11px] text-warn">
              <AlertTriangle size={11} /> {account.usageError}
            </span>
          )}
          {account.usageFetchedAt && (
            <span className="ml-auto text-[10px] ink-3">{relativeTime(account.usageFetchedAt, now)}</span>
          )}
        </div>
      )}
    </section>
  );
}

function AccountRow({
  account,
  threshold,
  busy,
  onSwitch,
  onReauth,
  onRemove,
}: {
  account: AccountView;
  threshold: number;
  busy: string | null;
  onSwitch: () => void;
  onReauth: () => void;
  onRemove: () => void;
}) {
  const [confirmRemove, setConfirmRemove] = useState(false);
  const v = bindingPct(account.usage);
  const t = tone(v, threshold);
  const switching = busy === `switch:${account.id}`;
  const canSwitch = !account.isActive && account.hasBackup && !account.needsReauth && !busy;

  return (
    <motion.li
      layout
      initial={{ opacity: 0, y: 6 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, scale: 0.98 }}
      transition={{ type: "spring", stiffness: 400, damping: 30 }}
      className={`group panel relative flex items-center gap-3 rounded-2xl px-3 py-2.5 transition-colors ${
        account.isActive ? "ring-1 ring-accent/40" : "hover:panel-hover"
      }`}
    >
      <button
        type="button"
        disabled={!canSwitch}
        onClick={onSwitch}
        className="focus-ring flex min-w-0 flex-1 items-center gap-3 rounded-xl text-left disabled:cursor-default"
        title={canSwitch ? `Switch to ${account.email}` : undefined}
      >
        <Avatar account={account} size={30} />
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-1.5">
            <span className="truncate text-[13px] font-medium">{account.email}</span>
            {account.isActive && <Check size={13} className="shrink-0 text-accent" />}
          </div>
          <div className="mt-1 flex items-center gap-2">
            <div className="w-24">
              <UsageBar value={v} tone={t} size="sm" />
            </div>
            <span className={`text-[11px] tnum ${t === "critical" ? "text-critical" : t === "warn" ? "text-warn" : "ink-3"}`}>
              {v === null ? "–" : `${Math.round(v)}%`}
            </span>
            {planLabel(account) && <span className="text-[11px] ink-3">· {planLabel(account)}</span>}
            {account.needsReauth && <span className="text-[11px] text-warn">· needs login</span>}
          </div>
        </div>
      </button>

      <div className="flex shrink-0 items-center gap-0.5">
        {confirmRemove ? (
          <>
            <button
              type="button"
              onClick={() => {
                setConfirmRemove(false);
                onRemove();
              }}
              className="rounded-full bg-critical/15 px-2 py-1 text-[11px] font-semibold text-critical hover:bg-critical/25"
            >
              Remove
            </button>
            <IconButton label="Cancel" className="h-7 w-7" onClick={() => setConfirmRemove(false)}>
              <X size={13} />
            </IconButton>
          </>
        ) : (
          <>
            <span className="flex items-center gap-0.5 opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100">
              <IconButton label="Re-authenticate" className="h-7 w-7" disabled={!!busy} onClick={onReauth}>
                <KeyRound size={13} />
              </IconButton>
              <IconButton label="Remove account" className="h-7 w-7" disabled={!!busy} onClick={() => setConfirmRemove(true)}>
                <Trash2 size={13} />
              </IconButton>
            </span>
            {!account.isActive && (
              <IconButton label="Switch to this account" className="h-7 w-7 text-accent" disabled={!canSwitch} onClick={onSwitch}>
                {switching ? <Spinner className="h-3.5 w-3.5" /> : <ArrowRightLeft size={14} />}
              </IconButton>
            )}
          </>
        )}
      </div>
    </motion.li>
  );
}

function EmptyState({ cliAvailable, busy, onAdd }: { cliAvailable: boolean; busy: boolean; onAdd: () => void }) {
  return (
    <section className="panel flex flex-col items-center rounded-xl2 px-5 py-7 text-center">
      <div className="mb-3 flex h-14 w-14 items-center justify-center rounded-2xl bg-gradient-to-br from-accent-soft/25 to-accent/25 text-accent">
        <ArrowRightLeft size={22} />
      </div>
      <h2 className="text-[15px] font-semibold">Keep every Claude login</h2>
      <p className="mt-1 max-w-[260px] text-[12px] leading-relaxed ink-2">
        Add each account once. Switching later takes one click and never signs you out.
      </p>
      <Button className="mt-4" disabled={!cliAvailable || busy} onClick={onAdd}>
        {busy ? <Spinner className="h-3.5 w-3.5 border-white/40 border-t-white" /> : <Plus size={14} />}
        Add account
      </Button>
    </section>
  );
}
