import { invoke } from "@tauri-apps/api/core";
import type {
  Account,
  BiometricStatus,
  BookmarkList,
  BrowserChannel,
  BrowserList,
  ChangelogEntry,
  Config,
  Hotkeys,
  PlatformStatus,
  RadialConfig,
  SelectorProfile,
  SecurityState,
  Site,
  UpdateInfo,
} from "./types";

export async function call<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  return invoke<T>(command, args);
}

export const api = {
  securityState: () => call<SecurityState>("security_state"),
  securitySetup: (passphrase: string) =>
    call<void>("security_setup", { passphrase }),
  securityUnlock: (passphrase: string) =>
    call<void>("security_unlock", { passphrase }),
  securityUnlockBiometric: () => call<void>("security_unlock_biometric"),
  securityLock: () => call<void>("security_lock"),
  securityChangePassphrase: (current: string, next: string) =>
    call<void>("security_change_passphrase", { current, next }),
  securityReset: () => call<void>("security_reset"),
  biometricStatus: () => call<BiometricStatus>("security_biometric_status"),
  biometricEnable: (passphrase: string) =>
    call<void>("security_biometric_enable", { passphrase }),
  biometricDisable: () => call<void>("security_biometric_disable"),

  configRead: () => call<Config>("config_read"),
  configUpdateSettings: (settings: Partial<Config["settings"]>) =>
    call<Config>("config_update_settings", { settings }),
  configExport: (path?: string) =>
    call<string>("config_export", { path: path ?? null }),
  configImport: (path: string) => call<Config>("config_import", { path }),

  sitesSave: (site: Site) => call<Site>("sites_save", { site }),
  sitesDelete: (id: string) => call<void>("sites_delete", { id }),
  sitesReorder: (ids: string[]) => call<void>("sites_reorder", { ids }),
  accountsSave: (account: Account, password?: string) =>
    call<Account>("accounts_save", { account, password: password ?? null }),
  accountsDelete: (id: string) => call<void>("accounts_delete", { id }),
  groupsAdd: (name: string) => call<void>("groups_add", { name }),
  groupsRename: (from: string, to: string) =>
    call<void>("groups_rename", { from, to }),
  groupsDelete: (name: string) => call<void>("groups_delete", { name }),

  vaultHas: (empNo: string) => call<boolean>("vault_has", { empNo }),
  vaultSet: (empNo: string, password: string) =>
    call<void>("vault_set", { empNo, password }),
  vaultClear: (empNo: string) => call<void>("vault_clear", { empNo }),
  sharedGet: () => call<string | null>("shared_get"),
  sharedSet: (password: string) => call<void>("shared_set", { password }),

  browsersList: () => call<BrowserList>("browsers_list"),
  browsersSelect: (channel: BrowserChannel, executablePath: string | null) =>
    call<void>("browsers_select", { channel, executablePath }),
  bookmarksList: () => call<BookmarkList>("bookmarks_list"),
  bookmarksKeywordsAdd: (keyword: string) =>
    call<string[]>("bookmarks_keywords_add", { keyword }),
  bookmarksKeywordsRemove: (keyword: string) =>
    call<string[]>("bookmarks_keywords_remove", { keyword }),
  openFolder: (kind: "downloads" | "data") =>
    call<string>("open_folder", { kind }),

  loginStart: (siteId: string | null, accountId: string | null, urlOverride?: string | null) =>
    call<string>("login_start", { siteId, accountId, urlOverride: urlOverride ?? null }),
  loginCancel: (sessionId: string) =>
    call<void>("login_cancel", { sessionId }),
  loginOpen: (url: string) => call<string>("login_open", { url }),
  learnStart: (siteId: string | null, loginUrl: string | null) =>
    call<string>("learn_start", { siteId, loginUrl }),
  learnCancel: (sessionId: string) =>
    call<void>("learn_cancel", { sessionId }),
  learnAccept: (sessionId: string, selectors: SelectorProfile) =>
    call<SelectorProfile>("learn_accept", { sessionId, selectors }),

  windowGetExpanded: () => call<boolean>("window_get_expanded"),
  windowSetExpanded: (expanded: boolean) =>
    call<void>("window_set_expanded", { expanded }),
  windowDragStart: () => call<void>("window_drag_start"),
  windowShow: (expanded: boolean) => call<void>("window_show", { expanded }),

  hotkeysGet: () => call<Hotkeys>("hotkeys_get"),
  hotkeysSet: (togglePanel: string, lock: string) =>
    call<void>("hotkeys_set", { togglePanel, lock }),
  radialGet: () => call<RadialConfig>("radial_get"),
  radialSet: (radial: RadialConfig) => call<RadialConfig>("radial_set", { radial }),

  updateCheck: () => call<UpdateInfo | null>("update_check"),
  updateInstall: () => call<void>("update_install"),
  appVersion: () => call<string>("app_version"),
  appQuit: () => call<void>("app_quit"),
  autostartGet: () => call<boolean>("autostart_get"),
  autostartSet: (enabled: boolean) =>
    call<void>("autostart_set", { enabled }),
  changelog: () => call<ChangelogEntry[]>("changelog"),
  platformStatus: () => call<PlatformStatus>("platform_status"),
};
