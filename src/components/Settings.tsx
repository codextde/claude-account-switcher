import { useEffect, useRef, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Check, Download, ExternalLink, RefreshCw, Search } from "lucide-react";
import { api, errorMessage } from "../lib/api";
import { relativeTime } from "../lib/format";
import type { Settings as SettingsModel, Snapshot, TrayMode, TrayWindow, UpdateInfo } from "../lib/types";
import { Segmented, Spinner, Toggle } from "./ui";

const REPO_URL = "https://github.com/codextde/claude-account-switcher";

export default function Settings({ snapshot }: { snapshot: Snapshot | null }) {
  const [draft, setDraft] = useState<SettingsModel | null>(null);
  const [status, setStatus] = useState<"idle" | "saving" | "saved" | "error">("idle");
  const [error, setError] = useState<string | null>(null);
  const lastSaved = useRef<string>("");

  // Adopt backend settings until the user starts editing.
  useEffect(() => {
    if (!snapshot) return;
    const incoming = JSON.stringify(snapshot.settings);
    if (draft === null || incoming === lastSaved.current) {
      setDraft(snapshot.settings);
      lastSaved.current = incoming;
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [snapshot?.settings]);

  // Debounced autosave.
  useEffect(() => {
    if (!draft) return;
    const serialized = JSON.stringify(draft);
    if (serialized === lastSaved.current) return;
    const id = window.setTimeout(async () => {
      setStatus("saving");
      try {
        await api.updateSettings(draft);
        lastSaved.current = serialized;
        setStatus("saved");
        setError(null);
        window.setTimeout(() => setStatus("idle"), 1500);
      } catch (e) {
        setStatus("error");
        setError(errorMessage(e));
      }
    }, 350);
    return () => window.clearTimeout(id);
  }, [draft]);

  if (!snapshot || !draft) {
    return (
      <div className="flex h-full items-center justify-center">
        <Spinner />
      </div>
    );
  }

  const patch = (p: Partial<SettingsModel>) => setDraft({ ...draft, ...p });
  const isMac = snapshot.platform === "macos";

  return (
    <div className="mx-auto flex max-w-[560px] flex-col gap-6 px-7 py-7">
      <header className="flex items-center justify-between">
        <div>
          <h1 className="text-[20px] font-semibold tracking-tight">Settings</h1>
          <p className="text-[12px] ink-3">Changes save automatically.</p>
        </div>
        <div className="flex items-center gap-1.5 text-[12px] ink-3">
          {status === "saving" && (
            <>
              <Spinner className="h-3 w-3" /> Saving
            </>
          )}
          {status === "saved" && (
            <>
              <Check size={13} className="text-ok" /> Saved
            </>
          )}
          {status === "error" && <span className="text-critical">{error}</span>}
        </div>
      </header>

      <Section title="Automatic switching" description="Move to the account with the most headroom when the active one runs hot.">
        <Row label="Auto-switch" hint="Uses the higher of the 5-hour and 7-day windows.">
          <Toggle checked={draft.autoSwitch} onChange={(v) => patch({ autoSwitch: v })} label="Auto-switch" />
        </Row>
        <SliderRow
          label="Switch when usage reaches"
          value={draft.threshold}
          min={50}
          max={100}
          step={1}
          unit="%"
          disabled={!draft.autoSwitch}
          onChange={(v) => patch({ threshold: v })}
        />
        <SliderRow
          label="Target must be at least this far below"
          hint="Hysteresis keeps two busy accounts from ping-ponging."
          value={draft.hysteresis}
          min={0}
          max={50}
          step={1}
          unit="%"
          disabled={!draft.autoSwitch}
          onChange={(v) => patch({ hysteresis: v })}
        />
        <SliderRow
          label="Cooldown between automatic switches"
          value={Math.round(draft.switchCooldownSecs / 60)}
          min={0}
          max={120}
          step={1}
          unit=" min"
          disabled={!draft.autoSwitch}
          onChange={(v) => patch({ switchCooldownSecs: v * 60 })}
        />
        <Row label="Notify on switch">
          <Toggle checked={draft.notifications} onChange={(v) => patch({ notifications: v })} label="Notifications" />
        </Row>
      </Section>

      <Section title="Menu bar" description="What the tray icon shows for the active account.">
        <Row label="Display">
          <Segmented<TrayMode>
            value={draft.trayMode}
            onChange={(v) => patch({ trayMode: v })}
            options={
              isMac
                ? [
                    { value: "both", label: "Bar + %" },
                    { value: "bar", label: "Bar" },
                    { value: "percent", label: "%" },
                  ]
                : [
                    { value: "both", label: "Bar" },
                    { value: "percent", label: "Dot" },
                  ]
            }
          />
        </Row>
        <Row label="Window" hint="Which limit drives the bar.">
          <Segmented<TrayWindow>
            value={draft.trayWindow}
            onChange={(v) => patch({ trayWindow: v })}
            options={[
              { value: "max", label: "Highest" },
              { value: "five-hour", label: "5 hour" },
              { value: "seven-day", label: "7 day" },
            ]}
          />
        </Row>
        <SliderRow
          label="Refresh usage every"
          value={draft.pollIntervalSecs}
          min={30}
          max={600}
          step={30}
          unit=" s"
          onChange={(v) => patch({ pollIntervalSecs: v })}
        />
      </Section>

      <Section title="System">
        <Row label="Launch at login">
          <Toggle checked={draft.launchAtLogin} onChange={(v) => patch({ launchAtLogin: v })} label="Launch at login" />
        </Row>
        <Row label="Install updates automatically" hint="New releases download in the background and install when the app is idle.">
          <Toggle checked={draft.autoUpdate} onChange={(v) => patch({ autoUpdate: v })} label="Install updates automatically" />
        </Row>
        <UpdateRow update={snapshot.update} />
        <div className="flex flex-col gap-1.5 py-2">
          <div className="flex items-center justify-between">
            <span className="text-[13px] font-medium">Claude Code CLI</span>
            <span className={`text-[12px] ${snapshot.cli.available ? "text-ok" : "text-warn"}`}>
              {snapshot.cli.available ? snapshot.cli.version || "found" : "not found"}
            </span>
          </div>
          <div className="flex items-center gap-2">
            <input
              type="text"
              value={draft.cliPath ?? ""}
              onChange={(e) => patch({ cliPath: e.target.value || null })}
              placeholder={snapshot.cli.path ?? "Auto-detect (leave empty)"}
              spellCheck={false}
              className="focus-ring panel min-w-0 flex-1 rounded-xl px-3 py-2 font-mono text-[12px] ink placeholder:ink-3"
            />
            <button
              type="button"
              onClick={() => api.detectCli()}
              className="focus-ring panel inline-flex items-center gap-1.5 rounded-xl px-3 py-2 text-[12px] font-medium ink-2 hover:panel-hover hover:ink"
              title="Detect again"
            >
              <Search size={13} /> Detect
            </button>
          </div>
          <p className="text-[11px] ink-3">
            Used for <span className="font-mono">claude auth login</span> and <span className="font-mono">claude auth status</span>.
          </p>
        </div>
      </Section>

      <Section title="About">
        <div className="flex items-center justify-between py-1">
          <div>
            <div className="text-[13px] font-medium">Claude Account Switcher</div>
            <div className="text-[12px] ink-3">Version {snapshot.version} · {snapshot.platform}</div>
          </div>
          <div className="flex items-center gap-1">
            <button
              type="button"
              onClick={() => openUrl(REPO_URL)}
              className="focus-ring inline-flex items-center gap-1.5 rounded-full px-3 py-1.5 text-[12px] font-medium ink-2 hover:panel-hover hover:ink"
            >
              <ExternalLink size={13} /> GitHub
            </button>
            <button
              type="button"
              onClick={() => api.refreshUsage()}
              className="focus-ring inline-flex items-center gap-1.5 rounded-full px-3 py-1.5 text-[12px] font-medium ink-2 hover:panel-hover hover:ink"
            >
              <RefreshCw size={13} /> Refresh now
            </button>
          </div>
        </div>
        <p className="text-[11px] leading-relaxed ink-3">
          Credentials are stored locally with owner-only permissions in the app data folder and are only sent to Anthropic.
          Not affiliated with Anthropic.
        </p>
      </Section>
    </div>
  );
}

function UpdateRow({ update }: { update: UpdateInfo }) {
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<"current" | null>(null);
  const busy = update.stage === "checking" || update.stage === "downloading" || update.stage === "installing";

  const run = async (fn: () => Promise<unknown>) => {
    setError(null);
    setResult(null);
    try {
      const version = await fn();
      if (version === null) setResult("current");
    } catch (e) {
      setError(errorMessage(e));
    }
  };

  let status: string;
  switch (update.stage) {
    case "checking":
      status = "Checking for updates…";
      break;
    case "downloading":
      status = `Downloading ${update.version}…`;
      break;
    case "ready":
      status = `${update.version} is downloaded and installs when the app is idle.`;
      break;
    case "installing":
      status = `Installing ${update.version}…`;
      break;
    default:
      status =
        error ?? update.error ?? (result === "current" ? "You have the latest version." : update.lastCheckedAt ? `Last checked ${relativeTime(update.lastCheckedAt, Date.now())}.` : "Not checked yet.");
  }
  const tone = error || (update.stage === "idle" && update.error) ? "text-critical" : update.stage === "ready" ? "text-ok" : "ink-3";

  return (
    <div className="flex items-center justify-between gap-4 py-3">
      <div className="min-w-0">
        <div className="text-[13px] font-medium">Updates</div>
        <div className={`text-[11px] ${tone}`}>{status}</div>
      </div>
      {update.stage === "ready" ? (
        <button
          type="button"
          onClick={() => run(() => api.installUpdate())}
          className="focus-ring inline-flex shrink-0 items-center gap-1.5 rounded-full bg-accent px-3 py-1.5 text-[12px] font-medium text-white hover:opacity-90"
        >
          <Download size={13} /> Restart to update
        </button>
      ) : (
        <button
          type="button"
          disabled={busy}
          onClick={() => run(() => api.checkForUpdates())}
          className="focus-ring panel inline-flex shrink-0 items-center gap-1.5 rounded-xl px-3 py-2 text-[12px] font-medium ink-2 hover:panel-hover hover:ink disabled:opacity-50"
        >
          {busy ? <Spinner className="h-3 w-3" /> : <RefreshCw size={13} />} Check now
        </button>
      )}
    </div>
  );
}

function Section({ title, description, children }: { title: string; description?: string; children: React.ReactNode }) {
  return (
    <section>
      <h2 className="text-[11px] font-semibold uppercase tracking-[0.12em] ink-3">{title}</h2>
      {description && <p className="mt-0.5 text-[12px] ink-3">{description}</p>}
      <div className="panel mt-2.5 flex flex-col divide-y divide-[rgb(var(--line)/var(--line-alpha))] rounded-xl2 px-4">{children}</div>
    </section>
  );
}

function Row({ label, hint, children }: { label: string; hint?: string; children: React.ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-4 py-3">
      <div className="min-w-0">
        <div className="text-[13px] font-medium">{label}</div>
        {hint && <div className="text-[11px] ink-3">{hint}</div>}
      </div>
      {children}
    </div>
  );
}

function SliderRow({
  label,
  hint,
  value,
  min,
  max,
  step,
  unit,
  disabled,
  onChange,
}: {
  label: string;
  hint?: string;
  value: number;
  min: number;
  max: number;
  step: number;
  unit: string;
  disabled?: boolean;
  onChange: (v: number) => void;
}) {
  const ratio = ((value - min) / (max - min)) * 100;
  return (
    <div className={`py-3 ${disabled ? "opacity-40" : ""}`}>
      <div className="flex items-center justify-between">
        <div>
          <div className="text-[13px] font-medium">{label}</div>
          {hint && <div className="text-[11px] ink-3">{hint}</div>}
        </div>
        <span className="text-[13px] font-semibold tnum">
          {value}
          {unit}
        </span>
      </div>
      <input
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        disabled={disabled}
        onChange={(e) => onChange(Number(e.target.value))}
        className="range mt-2 w-full"
        style={{ ["--ratio" as string]: `${ratio}%` }}
      />
    </div>
  );
}
