import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  AlertTriangle,
  ArrowRightLeft,
  Download,
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
import type { Tone } from "../lib/format";
import UsageBar from "./UsageBar";
import { Badge, Button, IconButton, Spinner } from "./ui";

const pillCls =
  "focus-ring ml-0.5 inline-flex h-7 items-center gap-1 rounded-full bg-accent/15 px-2.5 text-[11px] font-semibold text-accent transition-colors hover:bg-accent/25 disabled:opacity-40";

const toneText: Record<Tone, string> = {
  ok: "ink",
  warn: "text-warn",
  critical: "text-critical",
};

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
  const canAdd = snapshot.cli.available && !snapshot.login.inProgress && busy !== "add";

  return (
    <Shell>
      {/* Header */}
      <header className="flex items-center gap-3 px-4 pt-4 pb-3">
        <Avatar account={active} size={34} />
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <span className="truncate text-[14px] font-semibold leading-tight">
              {active ? active.displayName || active.email : snapshot.unknownActiveEmail ? "Unknown account" : "No active account"}
            </span>
            {active && planLabel(active) && <Badge tone="accent">{planLabel(active)}</Badge>}
          </div>
          <div className="truncate text-[12px] leading-tight ink-3">
            {active ? active.email : snapshot.unknownActiveEmail ?? "Add an account to get started"}
          </div>
        </div>
        <div className="flex shrink-0 items-center">
          <IconButton
            label="Refresh usage"
            className="h-8 w-8"
            disabled={snapshot.refreshing}
            onClick={() => run("refresh", api.refreshUsage)}
          >
            <RefreshCw size={15} className={snapshot.refreshing ? "spin" : ""} />
          </IconButton>
          <IconButton label="Settings" className="h-8 w-8" onClick={() => api.openSettings()}>
            <SettingsIcon size={15} />
          </IconButton>
        </div>
      </header>

      <div className="min-h-0 flex-1 overflow-y-auto px-4 pb-4">
        {/* Alerts */}
        <AnimatePresence initial={false}>
          {!snapshot.cli.available && (
            <Notice key="cli" tone="warn" icon={<AlertTriangle size={14} />}>
              Claude Code CLI not found. Install it or set its path in Settings.
            </Notice>
          )}
          {snapshot.update.stage === "ready" && (
            <Notice key="update" tone="accent" icon={<Download size={14} />}>
              Version {snapshot.update.version} is ready.
              <button className="ml-1 font-semibold underline underline-offset-2" onClick={() => run("update", api.installUpdate)}>
                Restart to update
              </button>
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

        {/* Accounts */}
        {snapshot.accounts.length > 0 && (
          <section className="mt-5">
            <div className="mb-2 flex items-center justify-between px-1">
              <h2 className="text-[11px] font-semibold uppercase tracking-[0.1em] ink-3">
                Accounts
                <span className="ml-1.5 font-medium normal-case tracking-normal ink-3 opacity-70">{snapshot.accounts.length}</span>
              </h2>
              <button
                type="button"
                disabled={!canAdd}
                onClick={() => run("add", api.addAccount)}
                className="focus-ring -mr-1 inline-flex h-6 items-center gap-1 rounded-full px-2 text-[12px] font-medium text-accent transition-colors hover:bg-accent/10 disabled:opacity-40"
              >
                <Plus size={13} strokeWidth={2.5} /> Add
              </button>
            </div>
            <ul className="flex flex-col gap-1">
              <AnimatePresence initial={false}>
                {[...(active ? [active] : []), ...others].map((a) => (
                  <AccountRow
                    key={a.id}
                    account={a}
                    threshold={threshold}
                    busy={busy}
                    now={now}
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
      <footer className="flex items-center gap-2 border-t hairline px-4 py-2 text-[11px] ink-3">
        <span
          className={`inline-flex shrink-0 items-center gap-1 rounded-full px-1.5 py-0.5 font-medium ${
            snapshot.settings.autoSwitch ? "bg-accent/12 text-accent" : "track ink-3"
          }`}
        >
          <Zap size={11} strokeWidth={2.5} />
          {snapshot.settings.autoSwitch ? `Auto ${threshold}%` : "Auto off"}
        </span>
        <span className="min-w-0 flex-1 truncate">
          {snapshot.lastEvent
            ? snapshot.lastEvent.message
            : snapshot.lastRefreshAt
              ? `Updated ${relativeTime(snapshot.lastRefreshAt, now)}`
              : ""}
        </span>
        <IconButton label="Quit" className="-mr-1.5 h-7 w-7" onClick={() => api.quit()}>
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
      className="flex shrink-0 items-center justify-center rounded-full bg-gradient-to-br from-accent-soft to-accent font-bold text-white shadow-[inset_0_-2px_6px_rgba(0,0,0,0.25)]"
      style={{ width: size, height: size, fontSize: Math.round(size * 0.36) }}
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
  tone: "warn" | "critical" | "accent";
  icon: React.ReactNode;
  children: React.ReactNode;
  onClose?: () => void;
}) {
  const cls = t === "warn" ? "bg-warn/12 text-warn" : t === "accent" ? "bg-accent/12 text-accent" : "bg-critical/12 text-critical";
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
    { label: "Opus", window: usage?.sevenDayOpus ?? null },
    { label: "Sonnet", window: usage?.sevenDaySonnet ?? null },
  ].filter((m) => pct(m.window) !== null);

  return (
    <section className="panel rounded-xl2 px-4 py-3.5">
      <div className="flex flex-col gap-4">
        {rows.map((r) => {
          const v = pct(r.window);
          const t = tone(v, threshold);
          const reset = countdown(resetsAtMs(r.window), now);
          return (
            <div key={r.label}>
              <div className="mb-2 flex items-end justify-between">
                <div className="leading-none">
                  <div className="text-[13px] font-semibold">{r.label}</div>
                  <div className="mt-1 text-[11px] ink-3">{r.hint}</div>
                </div>
                <div className="text-right leading-none tnum">
                  <motion.div
                    key={v ?? "na"}
                    initial={{ opacity: 0.4 }}
                    animate={{ opacity: 1 }}
                    className={`text-[22px] font-semibold tracking-tight ${toneText[t]}`}
                  >
                    {v === null ? (loading ? "…" : "–") : `${Math.round(v)}%`}
                  </motion.div>
                  <div className="mt-1 text-[11px] ink-3">{reset ? `resets in ${reset}` : " "}</div>
                </div>
              </div>
              <UsageBar value={v} tone={t} elapsed={elapsedPct(r.window, r.span, now)} size="lg" loading={loading} />
            </div>
          );
        })}
      </div>

      {(models.length > 0 || account.usageError || account.usageFetchedAt) && (
        <div className="mt-3.5 flex flex-wrap items-center gap-x-3 gap-y-1.5 border-t hairline pt-3">
          {models.map((m) => {
            const v = pct(m.window)!;
            return (
              <span key={m.label} className="inline-flex items-center gap-1.5 text-[11px] ink-3 tnum">
                <span
                  className="inline-block h-1.5 w-1.5 rounded-full"
                  style={{ background: v >= threshold ? "var(--color-critical)" : v >= threshold - 20 ? "var(--color-warn)" : "var(--color-ok)" }}
                />
                {m.label} <span className="font-semibold ink-2">{Math.round(v)}%</span>
              </span>
            );
          })}
          {account.usageError && (
            <span className="inline-flex items-center gap-1 text-[11px] text-warn">
              <AlertTriangle size={11} /> {account.usageError}
            </span>
          )}
          {account.usageFetchedAt && (
            <span className="ml-auto text-[11px] ink-3">{relativeTime(account.usageFetchedAt, now)}</span>
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
  now,
  onSwitch,
  onReauth,
  onRemove,
}: {
  account: AccountView;
  threshold: number;
  busy: string | null;
  now: number;
  onSwitch: () => void;
  onReauth: () => void;
  onRemove: () => void;
}) {
  const [confirmRemove, setConfirmRemove] = useState(false);
  const v = bindingPct(account.usage);
  const t = tone(v, threshold);
  const switching = busy === `switch:${account.id}`;
  const reauthing = busy === `reauth:${account.id}`;
  const canSwitch = !account.isActive && account.hasBackup && !account.needsReauth && !busy;
  const plan = planLabel(account);

  const fiveHour = pct(account.usage?.fiveHour);
  const sevenDay = pct(account.usage?.sevenDay);
  const bindingWindow = v === null ? null : fiveHour !== null && fiveHour >= (sevenDay ?? -1) ? account.usage?.fiveHour : account.usage?.sevenDay;
  const reset = countdown(resetsAtMs(bindingWindow), now);

  const subline: React.ReactNode[] = [];
  if (account.isActive) subline.push(<span key="cur" className="text-accent">Current</span>);
  if (plan) subline.push(<span key="plan">{plan}</span>);
  if (account.needsReauth) subline.push(<span key="auth" className="text-warn">Needs login</span>);
  else if (!account.hasBackup) subline.push(<span key="nb" className="text-warn">No backup</span>);
  else if (!account.isActive && reset && v !== null) subline.push(<span key="reset">resets in {reset}</span>);

  // Width the text column gives up while the hover actions are shown, so they never cover the email.
  const reserve = confirmRemove ? 28 : account.isActive ? 0 : 72;

  return (
    <motion.li
      layout
      initial={{ opacity: 0, y: 6 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, scale: 0.98 }}
      transition={{ type: "spring", stiffness: 400, damping: 30 }}
      className={`group relative flex items-center rounded-xl transition-colors ${
        account.isActive
          ? "bg-accent/8 ring-1 ring-inset ring-accent/25"
          : "hover:bg-[rgb(var(--panel)/var(--panel-alpha))]"
      }`}
    >
      <button
        type="button"
        disabled={!canSwitch}
        onClick={onSwitch}
        className="focus-ring flex min-w-0 flex-1 items-center gap-3 rounded-xl py-2 pl-2.5 pr-3 text-left disabled:cursor-default"
        title={canSwitch ? `Switch to ${account.email}` : undefined}
      >
        <Avatar account={account} size={30} />
        <div
          className={`min-w-0 flex-1 transition-[margin] duration-150 group-hover:mr-(--reserve) group-focus-within:mr-(--reserve) ${
            confirmRemove ? "mr-(--reserve)" : ""
          }`}
          style={{ "--reserve": `${reserve}px` } as React.CSSProperties}
        >
          <div className="truncate text-[13px] font-medium leading-tight">{account.email}</div>
          <div className="mt-1 truncate text-[11px] leading-tight ink-3">
            {subline.map((s, i) => (
              <span key={i}>
                {i > 0 && <span className="mx-1 opacity-50">·</span>}
                {s}
              </span>
            ))}
          </div>
        </div>

        {/* Usage meter: fades out when the hover actions take its place */}
        <div
          className={`flex w-[68px] shrink-0 flex-col items-end gap-1.5 transition-opacity duration-150 group-hover:opacity-0 group-focus-within:opacity-0 ${
            confirmRemove ? "opacity-0" : ""
          }`}
        >
          <span className={`text-[13px] font-semibold leading-none tnum ${toneText[t]}`}>
            {v === null ? "–" : `${Math.round(v)}%`}
          </span>
          <div className="w-12">
            <UsageBar value={v} tone={t} size="sm" />
          </div>
        </div>
      </button>

      {/* Hover actions, layered over the meter so the row never reflows */}
      <div
        className={`absolute inset-y-0 right-2 flex items-center gap-0.5 transition-opacity duration-150 ${
          confirmRemove ? "opacity-100" : "pointer-events-none opacity-0 group-hover:pointer-events-auto group-hover:opacity-100 group-focus-within:pointer-events-auto group-focus-within:opacity-100"
        }`}
      >
        {confirmRemove ? (
          <>
            <button
              type="button"
              onClick={() => {
                setConfirmRemove(false);
                onRemove();
              }}
              className="focus-ring h-7 rounded-full bg-critical/15 px-2.5 text-[11px] font-semibold text-critical hover:bg-critical/25"
            >
              Remove
            </button>
            <IconButton label="Cancel" className="h-7 w-7" onClick={() => setConfirmRemove(false)}>
              <X size={13} />
            </IconButton>
          </>
        ) : (
          <>
            {!account.needsReauth && (
              <IconButton label="Re-authenticate" className="h-7 w-7" disabled={!!busy} onClick={onReauth}>
                <KeyRound size={13} />
              </IconButton>
            )}
            <IconButton label="Remove account" className="h-7 w-7" disabled={!!busy} onClick={() => setConfirmRemove(true)}>
              <Trash2 size={13} />
            </IconButton>
            {account.needsReauth ? (
              <button type="button" disabled={!!busy} onClick={onReauth} className={pillCls}>
                {reauthing ? <Spinner className="h-3 w-3" /> : <KeyRound size={12} strokeWidth={2.5} />}
                Log in
              </button>
            ) : (
              !account.isActive && (
                <button type="button" disabled={!canSwitch} onClick={onSwitch} className={pillCls}>
                  {switching ? <Spinner className="h-3 w-3" /> : <ArrowRightLeft size={12} strokeWidth={2.5} />}
                  Switch
                </button>
              )
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
