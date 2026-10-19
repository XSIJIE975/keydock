import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  AlertCircle,
  Bookmark,
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  FolderOpen,
  LogOut,
  Search,
  Settings,
  Shield,
  Layers,
  Plus,
} from "lucide-react";

import { api } from "@/lib/api";
import type {
  Account,
  BookmarkList,
  Config,
  LoginPhase,
  LoginStatusEvent,
  Site,
} from "@/lib/types";
import { accountMatches, errorMessage, groupLabel, shortHost } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Spinner } from "@/components/ui/spinner";
import { useToast } from "@/components/ui/toast";
import { AccountEditor } from "./AccountEditor";
import { BookmarksTab } from "./BookmarksTab";
import { SettingsTab } from "./SettingsTab";
import { SiteEditor } from "./SiteEditor";

type Tab = "sites" | "bookmarks" | "settings";

interface DockProps {
  config: Config;
  onConfigChanged: (config: Config) => void;
}

interface Feedback {
  phase: LoginPhase;
  siteId: string | null;
  accountId: string | null;
  message?: string;
}

type SearchResult =
  | { kind: "site"; site: Site }
  | { kind: "account"; account: Account }
  | { kind: "bookmark"; name: string; url: string; folder: string };

export function Dock({ config, onConfigChanged }: DockProps) {
  const toast = useToast();
  const [tab, setTab] = useState<Tab>("sites");
  const [query, setQuery] = useState("");
  const [selectedSiteId, setSelectedSiteId] = useState<string | null>(
    config.sites[0]?.id ?? null,
  );
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [feedback, setFeedback] = useState<Record<string, Feedback>>({});
  const [bookmarks, setBookmarks] = useState<BookmarkList | null>(null);
  const [siteEditor, setSiteEditor] = useState<Site | null>(null);
  const [siteEditorOpen, setSiteEditorOpen] = useState(false);
  const [accountEditorOpen, setAccountEditorOpen] = useState(false);
  const [editingAccount, setEditingAccount] = useState<Account | null>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const feedbackTimers = useRef<Record<string, number>>({});

  const selectedSite =
    config.sites.find((site) => site.id === selectedSiteId) ?? null;

  useEffect(() => {
    void refreshBookmarks();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    const unlisteners: UnlistenFn[] = [];
    void listen<LoginStatusEvent>("login:status", (event) => {
      const { sessionId, phase, message } = event.payload;
      setFeedback((current) => {
        const previous = current[sessionId];
        if (!previous) return current;
        return {
          ...current,
          [sessionId]: { ...previous, phase, message },
        };
      });
      if (phase === "success" || phase === "need-manual" || phase === "failed") {
        if (feedbackTimers.current[sessionId]) {
          window.clearTimeout(feedbackTimers.current[sessionId]);
        }
        const delay = phase === "success" ? 1400 : 3200;
        feedbackTimers.current[sessionId] = window.setTimeout(() => {
          setFeedback((current) => {
            const next = { ...current };
            delete next[sessionId];
            return next;
          });
          delete feedbackTimers.current[sessionId];
        }, delay);
      }
      if (phase === "need-manual" && message) toast.info(message);
      if (phase === "failed" && message) toast.error(message);
    }).then((unlisten) => unlisteners.push(unlisten));
    void listen<{ filename: string; path: string }>("download:done", (event) => {
      toast.success(`下载完成：${event.payload.filename}`);
    }).then((unlisten) => unlisteners.push(unlisten));
    return () => {
      unlisteners.forEach((unlisten) => unlisten());
      Object.values(feedbackTimers.current).forEach((timer) =>
        window.clearTimeout(timer),
      );
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        searchRef.current?.focus();
        searchRef.current?.select();
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  async function refreshBookmarks() {
    try {
      setBookmarks(await api.bookmarksList());
    } catch (cause) {
      toast.error(errorMessage(cause));
    }
  }

  async function refreshConfig() {
    try {
      onConfigChanged(await api.configRead());
    } catch (cause) {
      toast.error(errorMessage(cause));
    }
  }

  async function login(site: Site, account: Account | null) {
    try {
      const sessionId = await api.loginStart(
        site.id,
        account?.id ?? site.defaultAccountId ?? null,
        null,
      );
      setFeedback((current) => ({
        ...current,
        [sessionId]: {
          phase: "starting",
          siteId: site.id,
          accountId: account?.id ?? site.defaultAccountId ?? null,
        },
      }));
    } catch (cause) {
      toast.error(errorMessage(cause));
    }
  }

  function siteFeedback(siteId: string): Feedback | null {
    const values = Object.values(feedback).filter(
      (item) => item.siteId === siteId,
    );
    return values[values.length - 1] ?? null;
  }

  function accountFeedback(accountId: string): Feedback | null {
    const values = Object.values(feedback).filter(
      (item) => item.accountId === accountId,
    );
    return values[values.length - 1] ?? null;
  }

  const accounts = useMemo(() => {
    const group = selectedSite?.group ?? "";
    const filtered = config.accounts.filter((account) =>
      accountMatches(account, group),
    );
    return filtered.sort((left, right) => {
      const leftDefault = left.id === selectedSite?.defaultAccountId ? 0 : 1;
      const rightDefault = right.id === selectedSite?.defaultAccountId ? 0 : 1;
      return leftDefault - rightDefault;
    });
  }, [config.accounts, selectedSite]);

  const results = useMemo<SearchResult[]>(() => {
    const needle = query.trim().toLowerCase();
    if (!needle) return [];
    const siteResults: SearchResult[] = config.sites
      .filter(
        (site) =>
          site.name.toLowerCase().includes(needle) ||
          site.loginUrl.toLowerCase().includes(needle) ||
          site.group.toLowerCase().includes(needle),
      )
      .map((site) => ({ kind: "site", site }));
    const accountResults: SearchResult[] = config.accounts
      .filter(
        (account) =>
          account.label.toLowerCase().includes(needle) ||
          account.empNo.toLowerCase().includes(needle) ||
          account.note.toLowerCase().includes(needle),
      )
      .map((account) => ({ kind: "account", account }));
    const bookmarkResults: SearchResult[] = (bookmarks?.items ?? [])
      .filter(
        (bookmark) =>
          bookmark.name.toLowerCase().includes(needle) ||
          bookmark.url.toLowerCase().includes(needle),
      )
      .slice(0, 8)
      .map((bookmark) => ({
        kind: "bookmark",
        name: bookmark.name,
        url: bookmark.url,
        folder: bookmark.folder,
      }));
    return [...siteResults, ...accountResults, ...bookmarkResults].slice(0, 12);
  }, [query, config, bookmarks]);

  function handleSiteClick(site: Site) {
    if (site.defaultAccountId) {
      const account =
        config.accounts.find((item) => item.id === site.defaultAccountId) ??
        null;
      void login(site, account);
    } else {
      setSelectedSiteId(site.id);
    }
  }

  function executeResult(result: SearchResult) {
    if (result.kind === "site") {
      handleSiteClick(result.site);
    } else if (result.kind === "account") {
      if (selectedSite) {
        void login(selectedSite, result.account);
      } else {
        toast.info("先在站点列表中选择一个站点");
      }
    } else {
      void api
        .loginOpen(result.url)
        .then(() => toast.info(`正在打开 ${shortHost(result.url)}`))
        .catch((cause) => toast.error(errorMessage(cause)));
    }
  }

  const groups = useMemo(() => {
    const names = new Set<string>();
    for (const site of config.sites) names.add(site.group);
    return Array.from(names);
  }, [config.sites]);

  return (
    <div className="kd-panel kd-enter flex h-full w-full flex-col overflow-hidden rounded-2xl">
      <div className="flex shrink-0 items-center gap-1.5 px-2.5 pt-2.5">
        <div className="relative flex-1">
          <Search className="pointer-events-none absolute left-2 top-2.5 h-3.5 w-3.5 text-muted" />
          <Input
            ref={searchRef}
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && results[0]) {
                event.preventDefault();
                executeResult(results[0]);
              }
              if (event.key === "Escape") setQuery("");
            }}
            className="h-8 pl-7 pr-12 text-[12px]"
            placeholder="搜索站点、账号、书签"
          />
          <span className="pointer-events-none absolute right-2 top-2.5 text-[10px] text-muted/70">
            Ctrl K
          </span>
        </div>
        <Button
          size="icon-sm"
          variant="ghost"
          aria-label="收起面板"
          onClick={() => void api.windowSetExpanded(false)}
        >
          <ChevronDown className="h-4 w-4" />
        </Button>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto px-2.5 py-2">
        {query ? (
          <div className="space-y-0.5">
            {results.length === 0 ? (
              <div className="py-8 text-center text-[12px] text-muted">
                没有匹配结果
              </div>
            ) : (
              results.map((result, index) => (
                <button
                  key={`${result.kind}-${index}`}
                  type="button"
                  onClick={() => executeResult(result)}
                  className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left hover:bg-elev"
                >
                  {result.kind === "site" ? (
                    <Layers className="h-3.5 w-3.5 shrink-0 text-accent" />
                  ) : result.kind === "account" ? (
                    <Shield className="h-3.5 w-3.5 shrink-0 text-muted" />
                  ) : (
                    <Bookmark className="h-3.5 w-3.5 shrink-0 text-muted" />
                  )}
                  <span className="min-w-0 flex-1 truncate">
                    {result.kind === "site"
                      ? result.site.name
                      : result.kind === "account"
                        ? `${result.account.label}（${result.account.empNo}）`
                        : result.name}
                  </span>
                  <span className="max-w-[120px] truncate text-[10px] text-muted">
                    {result.kind === "site"
                      ? groupLabel(result.site.group)
                      : result.kind === "account"
                        ? groupLabel(result.account.group)
                        : result.folder}
                  </span>
                </button>
              ))
            )}
          </div>
        ) : tab === "sites" ? (
          <div className="space-y-1">
            {config.sites.length === 0 ? (
              <div className="mt-10 flex flex-col items-center gap-2 text-center text-[12px] text-muted">
                <Layers className="h-6 w-6 opacity-50" />
                还没有站点
                <Button
                  size="sm"
                  variant="secondary"
                  onClick={() => {
                    setSiteEditor(null);
                    setSiteEditorOpen(true);
                  }}
                >
                  <Plus className="h-3.5 w-3.5" />
                  添加站点
                </Button>
              </div>
            ) : (
              groups.map((group) => {
                const sites = config.sites.filter((site) => site.group === group);
                const isCollapsed = collapsed.has(group);
                return (
                  <div key={group || "__none"}>
                    <button
                      type="button"
                      onClick={() =>
                        setCollapsed((current) => {
                          const next = new Set(current);
                          if (next.has(group)) next.delete(group);
                          else next.add(group);
                          return next;
                        })
                      }
                      className="flex w-full items-center gap-1 px-1.5 py-1 text-[11px] font-medium text-muted hover:text-fg"
                    >
                      {isCollapsed ? (
                        <ChevronRight className="h-3 w-3" />
                      ) : (
                        <ChevronDown className="h-3 w-3" />
                      )}
                      {groupLabel(group)}
                      <span className="ml-1 text-[10px] opacity-70">{sites.length}</span>
                    </button>
                    {!isCollapsed
                      ? sites.map((site) => {
                          const active = site.id === selectedSiteId;
                          const state = siteFeedback(site.id);
                          return (
                            <div
                              key={site.id}
                              className={`group flex items-center gap-1 rounded-md ${
                                active ? "bg-accent/12" : "hover:bg-elev"
                              }`}
                            >
                              <button
                                type="button"
                                onClick={() => {
                                  setSelectedSiteId(site.id);
                                  handleSiteClick(site);
                                }}
                                className="flex min-w-0 flex-1 items-center gap-2 px-2 py-1.5 text-left"
                              >
                                <span
                                  className={`h-1.5 w-1.5 shrink-0 rounded-full ${
                                    active ? "bg-accent" : "bg-zinc-600"
                                  }`}
                                />
                                <span className="min-w-0 flex-1 truncate text-[13px]">
                                  {site.name}
                                </span>
                                {state ? <FeedbackDot feedback={state} /> : null}
                              </button>
                              <button
                                type="button"
                                aria-label="编辑站点"
                                className="mr-1 hidden rounded p-1 text-muted hover:bg-line group-hover:block"
                                onClick={() => {
                                  setSiteEditor(site);
                                  setSiteEditorOpen(true);
                                }}
                              >
                                <Settings className="h-3 w-3" />
                              </button>
                            </div>
                          );
                        })
                      : null}
                  </div>
                );
              })
            )}
            {config.sites.length > 0 ? (
              <button
                type="button"
                onClick={() => {
                  setSiteEditor(null);
                  setSiteEditorOpen(true);
                }}
                className="flex w-full items-center justify-center gap-1 rounded-md border border-dashed border-line/70 py-1.5 text-[11px] text-muted hover:border-accent/40 hover:text-accent"
              >
                <Plus className="h-3 w-3" />
                添加站点
              </button>
            ) : null}
          </div>
        ) : tab === "bookmarks" ? (
          <BookmarksTab
            config={config}
            bookmarks={bookmarks}
            onConfigChanged={(next) => {
              onConfigChanged(next);
              void refreshBookmarks();
            }}
            onLogin={(url, accountId) => {
              void api
                .loginStart(null, accountId, url)
                .then((sessionId) => {
                  setFeedback((current) => ({
                    ...current,
                    [sessionId]: {
                      phase: "starting",
                      siteId: null,
                      accountId,
                    },
                  }));
                })
                .catch((cause) => toast.error(errorMessage(cause)));
            }}
            onOpen={(url) => {
              void api
                .loginOpen(url)
                .then(() => toast.info(`正在打开 ${shortHost(url)}`))
                .catch((cause) => toast.error(errorMessage(cause)));
            }}
          />
        ) : (
          <SettingsTab
            config={config}
            onConfigChanged={onConfigChanged}
            onEditAccount={(account) => {
              setEditingAccount(account);
              setAccountEditorOpen(true);
            }}
            onAddAccount={() => {
              setEditingAccount(null);
              setAccountEditorOpen(true);
            }}
          />
        )}
      </div>

      {tab === "sites" && !query ? (
        <div className="shrink-0 border-t border-line/70 px-2.5 py-2">
          <div className="mb-1.5 flex items-center justify-between text-[11px] text-muted">
            <span>登录为</span>
            <button
              type="button"
              className="text-accent hover:underline"
              onClick={() => setTab("settings")}
            >
              + 管理
            </button>
          </div>
          {!selectedSite ? (
            <div className="text-[11px] text-muted/80">选择站点后显示账号</div>
          ) : accounts.length === 0 ? (
            <div className="text-[11px] text-muted">
              该分组没有账号，
              <button
                type="button"
                className="text-accent hover:underline"
                onClick={() => {
                  setEditingAccount(null);
                  setAccountEditorOpen(true);
                }}
              >
                去管理
              </button>
            </div>
          ) : (
            <div className="flex flex-wrap gap-1.5">
              {accounts.map((account) => {
                const state = accountFeedback(account.id);
                return (
                  <button
                    key={account.id}
                    type="button"
                    onClick={() => void login(selectedSite, account)}
                    className="flex items-center gap-1.5 rounded-full border border-line bg-elev px-2.5 py-1 text-[11px] hover:border-accent/50 hover:text-accent"
                  >
                    {account.label}
                    {state ? <FeedbackDot feedback={state} compact /> : null}
                  </button>
                );
              })}
            </div>
          )}
        </div>
      ) : null}

      <div className="flex h-10 shrink-0 items-center gap-0.5 border-t border-line/70 px-1.5">
        <TabButton
          active={tab === "sites"}
          onClick={() => setTab("sites")}
          icon={<Layers className="h-3.5 w-3.5" />}
          label="站点"
        />
        <TabButton
          active={tab === "bookmarks"}
          onClick={() => setTab("bookmarks")}
          icon={<Bookmark className="h-3.5 w-3.5" />}
          label="书签"
        />
        <TabButton
          active={tab === "settings"}
          onClick={() => setTab("settings")}
          icon={<Settings className="h-3.5 w-3.5" />}
          label="设置"
        />
        <div className="flex-1" />
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label="下载文件夹"
          title="打开下载文件夹"
          onClick={() =>
            void api.openFolder("downloads").catch((cause) => toast.error(errorMessage(cause)))
          }
        >
          <FolderOpen className="h-4 w-4" />
        </Button>
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label="退出"
          title="退出 KeyDock"
          onClick={() => void api.appQuit()}
        >
          <LogOut className="h-4 w-4" />
        </Button>
      </div>

      <SiteEditor
        key={siteEditor?.id ?? "new-site"}
        open={siteEditorOpen}
        config={config}
        site={siteEditor}
        onClose={() => setSiteEditorOpen(false)}
        onSaved={() => {
          setSiteEditorOpen(false);
          void refreshConfig();
        }}
      />
      <AccountEditor
        key={editingAccount?.id ?? "new-account"}
        open={accountEditorOpen}
        config={config}
        account={editingAccount}
        onClose={() => setAccountEditorOpen(false)}
        onSaved={() => void refreshConfig()}
      />
    </div>
  );
}

function FeedbackDot({
  feedback,
  compact,
}: {
  feedback: Feedback;
  compact?: boolean;
}) {
  if (feedback.phase === "success") {
    return (
      <CheckCircle2
        className={`${compact ? "h-3 w-3" : "h-3.5 w-3.5"} text-accent`}
      />
    );
  }
  if (feedback.phase === "need-manual") {
    return (
      <AlertCircle
        className={`${compact ? "h-3 w-3" : "h-3.5 w-3.5"} text-warn`}
      />
    );
  }
  if (feedback.phase === "failed") {
    return (
      <AlertCircle
        className={`${compact ? "h-3 w-3" : "h-3.5 w-3.5"} text-danger`}
      />
    );
  }
  return <Spinner className="text-accent" />;
}

function TabButton({
  active,
  onClick,
  icon,
  label,
}: {
  active: boolean;
  onClick: () => void;
  icon: ReactNode;
  label: string;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={`flex h-8 items-center gap-1 rounded-md px-2 text-[12px] transition-colors ${
        active
          ? "bg-accent/12 text-accent"
          : "text-muted hover:bg-elev hover:text-fg"
      }`}
    >
      {icon}
      {label}
    </button>
  );
}
