import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";
import type { Settings, Snapshot } from "./types";

export const api = {
  getSnapshot: () => invoke<Snapshot>("get_snapshot"),
  refreshUsage: () => invoke<void>("refresh_usage"),
  addAccount: () => invoke<string>("add_account"),
  cancelLogin: () => invoke<void>("cancel_login"),
  adoptCurrent: () => invoke<string>("adopt_current"),
  switchAccount: (id: string) => invoke<void>("switch_account", { id }),
  removeAccount: (id: string) => invoke<void>("remove_account", { id }),
  reauthenticate: (id: string) => invoke<void>("reauthenticate", { id }),
  updateSettings: (settings: Settings) => invoke<void>("update_settings", { settings }),
  detectCli: () => invoke<void>("detect_cli"),
  openSettings: () => invoke<void>("open_settings"),
  hidePopover: () => invoke<void>("hide_popover"),
  resizePopover: (height: number) => invoke<void>("resize_popover", { height }),
  quit: () => invoke<void>("quit_app"),
};

/** Live view of backend state: fetched once, then updated by the `snapshot` event. */
export function useSnapshot(): Snapshot | null {
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);

  useEffect(() => {
    let disposed = false;
    api.getSnapshot()
      .then((s) => !disposed && setSnapshot(s))
      .catch((e) => console.error("get_snapshot failed", e));
    const unlisten = listen<Snapshot>("snapshot", (event) => {
      if (!disposed) setSnapshot(event.payload);
    });
    return () => {
      disposed = true;
      unlisten.then((fn) => fn()).catch(() => undefined);
    };
  }, []);

  return snapshot;
}

/** Re-renders on an interval so countdowns stay current. */
export function useTick(ms = 30_000): number {
  const [tick, setTick] = useState(Date.now());
  useEffect(() => {
    const id = window.setInterval(() => setTick(Date.now()), ms);
    return () => window.clearInterval(id);
  }, [ms]);
  return tick;
}

export function errorMessage(e: unknown): string {
  if (typeof e === "string") return e;
  if (e && typeof e === "object" && "message" in e) return String((e as { message: unknown }).message);
  return String(e);
}
