export type TrayMode = "both" | "bar" | "percent";
export type TrayWindow = "five-hour" | "seven-day" | "max";

export interface Settings {
  autoSwitch: boolean;
  threshold: number;
  hysteresis: number;
  pollIntervalSecs: number;
  switchCooldownSecs: number;
  launchAtLogin: boolean;
  notifications: boolean;
  autoUpdate: boolean;
  trayMode: TrayMode;
  trayWindow: TrayWindow;
  cliPath: string | null;
}

export interface UsageWindow {
  utilization: number | null;
  resetsAt: string | null;
}

export interface ExtraUsage {
  isEnabled: boolean | null;
  monthlyLimit: number | null;
  usedCredits: number | null;
  utilization: number | null;
}

export interface Usage {
  fiveHour: UsageWindow | null;
  sevenDay: UsageWindow | null;
  sevenDayOpus: UsageWindow | null;
  sevenDaySonnet: UsageWindow | null;
  extraUsage: ExtraUsage | null;
}

export interface AccountView {
  id: string;
  email: string;
  displayName: string | null;
  organizationName: string | null;
  subscriptionType: string | null;
  rateLimitTier: string | null;
  addedAt: number;
  lastActiveAt: number | null;
  isActive: boolean;
  hasBackup: boolean;
  needsReauth: boolean;
  usage: Usage | null;
  usageError: string | null;
  usageFetchedAt: number | null;
  tokenExpiresAt: number | null;
}

export interface CliInfo {
  available: boolean;
  path: string | null;
  version: string | null;
}

export interface LoginState {
  inProgress: boolean;
  url: string | null;
}

export interface ActivityEvent {
  kind: string;
  message: string;
  at: number;
}

export type UpdateStage = "idle" | "checking" | "downloading" | "ready" | "installing";

export interface UpdateInfo {
  stage: UpdateStage;
  version: string | null;
  lastCheckedAt: number | null;
  error: string | null;
}

export interface Snapshot {
  accounts: AccountView[];
  activeId: string | null;
  unknownActiveEmail: string | null;
  settings: Settings;
  cli: CliInfo;
  refreshing: boolean;
  lastRefreshAt: number | null;
  login: LoginState;
  lastEvent: ActivityEvent | null;
  update: UpdateInfo;
  platform: string;
  version: string;
}
