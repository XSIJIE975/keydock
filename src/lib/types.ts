export type SecurityState =
  | "none"
  | "setupRequired"
  | "locked"
  | "unlocked"
  | "legacy";

export type BrowserChannel =
  | "auto"
  | "chrome"
  | "msedge"
  | "chromium"
  | "brave"
  | "360"
  | "qq";

export interface SelectorProfile {
  frame: string | null;
  username: string | null;
  password: string | null;
  captcha: string | null;
  submit: string | null;
  success: string | null;
}

export type CaptchaMode = "off" | "fixed" | "ocr" | "manual";

export interface CaptchaConfig {
  mode: CaptchaMode;
  expectedLength: number | null;
  fixedValue: string | null;
  maxRetries: number;
}

export interface Site {
  id: string;
  name: string;
  group: string;
  loginUrl: string;
  selectors: SelectorProfile;
  captcha: CaptchaConfig | null;
  defaultAccountId: string | null;
  autoSubmit: boolean | null;
}

export interface Account {
  id: string;
  label: string;
  empNo: string;
  group: string;
  note: string;
}

export interface WindowSize {
  width: number;
  height: number;
}

export interface Settings {
  browser: {
    channel: BrowserChannel;
    executablePath: string | null;
    windowSize: WindowSize;
  };
  behavior: {
    autoSubmit: boolean;
    keepWindowOnFail: boolean;
    showBall: boolean;
  };
  captcha: CaptchaConfig;
  selectors: SelectorProfile;
  bookmarks: {
    keywords: string[];
  };
  security: {
    idleLockMinutes: number;
    biometricUnlock: boolean;
  };
}

export interface Hotkeys {
  togglePanel: string;
  lock: string;
}

export type RadialTrigger = "mouse4" | "mouse5" | "middle" | "disabled";

export type RadialAction =
  | { kind: "openPanel" }
  | { kind: "loginSite"; siteId: string; accountId?: string | null }
  | { kind: "openSite"; siteId: string }
  | { kind: "openBookmark"; url: string; accountId?: string | null }
  | { kind: "openDownloads" }
  | { kind: "lock" };

export interface RadialConfig {
  trigger: RadialTrigger;
  holdMs: number;
  sectors: (RadialAction | null)[];
}

export interface Config {
  version: number;
  settings: Settings;
  groups: string[];
  sites: Site[];
  accounts: Account[];
  hotkeys: Hotkeys;
  radial: RadialConfig;
}

export interface DetectedBrowser {
  id: string;
  name: string;
  channel: BrowserChannel;
  path: string;
  debugPortReliable: boolean;
}

export interface BrowserList {
  browsers: DetectedBrowser[];
  channel: BrowserChannel;
  executablePath: string | null;
}

export interface Bookmark {
  id: string;
  name: string;
  url: string;
  folder: string;
  browser: string;
  profile: string;
}

export interface BookmarkList {
  items: Bookmark[];
  sources: number;
  errors: string[];
}

export interface BiometricStatus {
  available: boolean;
  enabled: boolean;
  kind: string;
  experimental: boolean;
}

export interface PlatformStatus {
  os: string;
  arch: string;
  radialSupported: boolean;
  radialMessage: string;
  biometricAvailable: boolean;
  biometricKind: string;
  biometricExperimental: boolean;
  dataDir: string;
  downloadsDir: string;
  version: string;
}

export interface UpdateInfo {
  version: string;
  currentVersion: string;
  notes: string | null;
  date: string | null;
}

export interface ChangelogEntry {
  version: string;
  date: string;
  notes: string[];
}

export interface RadialSectorView {
  action: RadialAction | null;
  label: string;
  sublabel: string;
}

export type LoginPhase =
  | "starting"
  | "filling"
  | "submitting"
  | "success"
  | "need-manual"
  | "failed";

export interface LoginStatusEvent {
  sessionId: string;
  phase: LoginPhase;
  message?: string;
}

export interface LearnStatusEvent {
  sessionId: string;
  phase: "starting" | "capturing" | "captured" | "timeout" | "failed" | "cancelled";
  message?: string;
  captured?: SelectorProfile;
  landingUrl?: string;
}

export interface UpdateProgressEvent {
  downloaded: number;
  total: number | null;
}

export interface CmdError {
  code: string;
  message: string;
}

export type ToastKind = "info" | "success" | "error";
