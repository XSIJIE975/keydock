import { useMemo, useState } from "react";
import { ExternalLink, FolderOpen, Globe2, Plus, Search, X } from "lucide-react";

import { api } from "@/lib/api";
import type { Bookmark, BookmarkList, Config } from "@/lib/types";
import { errorMessage, groupLabel, shortHost } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { Dialog } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { useToast } from "@/components/ui/toast";

interface BookmarksTabProps {
  config: Config;
  bookmarks: BookmarkList | null;
  onConfigChanged: (config: Config) => void;
  onLogin: (url: string, accountId: string | null) => void;
  onOpen: (url: string) => void;
}

export function BookmarksTab({
  config,
  bookmarks,
  onConfigChanged,
  onLogin,
  onOpen,
}: BookmarksTabProps) {
  const toast = useToast();
  const [query, setQuery] = useState("");
  const [keyword, setKeyword] = useState("");
  const [selected, setSelected] = useState<Bookmark | null>(null);
  const [accountId, setAccountId] = useState<string>("");

  const keywords = config.settings.bookmarks.keywords;

  const items = useMemo(() => {
    const needle = query.trim().toLowerCase();
    return (bookmarks?.items ?? []).filter((bookmark) => {
      const haystack = `${bookmark.name} ${bookmark.url}`.toLowerCase();
      const keywordOk =
        keywords.length === 0 ||
        keywords.some((value) => haystack.includes(value.toLowerCase()));
      return keywordOk && (!needle || haystack.includes(needle));
    });
  }, [bookmarks, keywords, query]);

  async function addKeyword() {
    const value = keyword.trim();
    if (!value) return;
    try {
      await api.bookmarksKeywordsAdd(value);
      setKeyword("");
      onConfigChanged(await api.configRead());
    } catch (cause) {
      toast.error(errorMessage(cause));
    }
  }

  async function removeKeyword(value: string) {
    try {
      await api.bookmarksKeywordsRemove(value);
      onConfigChanged(await api.configRead());
    } catch (cause) {
      toast.error(errorMessage(cause));
    }
  }

  return (
    <div className="flex h-full flex-col">
      <div className="relative">
        <Search className="pointer-events-none absolute left-2 top-2.5 h-3.5 w-3.5 text-muted" />
        <Input
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          className="h-8 pl-7 text-[12px]"
          placeholder="搜索书签"
        />
      </div>

      <div className="mt-2 flex flex-wrap items-center gap-1">
        {keywords.map((value) => (
          <span
            key={value}
            className="flex items-center gap-1 rounded-full bg-accent/12 px-2 py-0.5 text-[10px] text-accent"
          >
            {value}
            <button
              type="button"
              aria-label={`移除 ${value}`}
              onClick={() => void removeKeyword(value)}
            >
              <X className="h-2.5 w-2.5" />
            </button>
          </span>
        ))}
        <div className="flex min-w-[110px] flex-1 items-center gap-1">
          <Input
            value={keyword}
            onChange={(event) => setKeyword(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") void addKeyword();
            }}
            placeholder={keywords.length ? "添加关键词" : "关键词为空 = 不过滤"}
            className="h-6 flex-1 text-[10px]"
          />
          <Button
            size="icon-sm"
            variant="ghost"
            aria-label="添加关键词"
            onClick={() => void addKeyword()}
          >
            <Plus className="h-3 w-3" />
          </Button>
        </div>
      </div>

      {bookmarks?.errors.length ? (
        <div className="mt-2 rounded-md border border-warn/25 bg-warn/10 px-2 py-1 text-[10px] text-warn">
          部分书签读取失败：{bookmarks.errors[0]}
        </div>
      ) : null}

      <div className="mt-2 min-h-0 flex-1 overflow-y-auto">
        {items.length === 0 ? (
          <div className="flex flex-col items-center gap-1 py-10 text-center text-[12px] text-muted">
            <Globe2 className="h-5 w-5 opacity-50" />
            {bookmarks === null
              ? "正在读取书签"
              : keywords.length
                ? "没有命中关键词的书签"
                : "没有书签"}
          </div>
        ) : (
          <div className="space-y-0.5">
            {items.map((bookmark) => (
              <button
                key={bookmark.id}
                type="button"
                onClick={() => {
                  setSelected(bookmark);
                  setAccountId("");
                }}
                className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left hover:bg-elev"
              >
                <FolderOpen className="h-3.5 w-3.5 shrink-0 text-muted" />
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-[12px]">{bookmark.name}</span>
                  <span className="block truncate text-[10px] text-muted">
                    {shortHost(bookmark.url)} · {bookmark.folder}
                  </span>
                </span>
                <ExternalLink className="h-3 w-3 shrink-0 text-muted" />
              </button>
            ))}
          </div>
        )}
      </div>

      <Dialog
        open={Boolean(selected)}
        title={selected?.name ?? "书签"}
        onClose={() => setSelected(null)}
        footer={
          <>
            <Button
              variant="ghost"
              size="sm"
              onClick={() => {
                if (!selected) return;
                onOpen(selected.url);
                setSelected(null);
              }}
            >
              只打开
            </Button>
            <Button
              size="sm"
              onClick={() => {
                if (!selected) return;
                onLogin(selected.url, accountId || null);
                setSelected(null);
              }}
            >
              打开并填表
            </Button>
          </>
        }
      >
        <div className="space-y-2">
          <div className="break-all rounded-md border border-line bg-elev/60 px-2 py-1.5 text-[11px] text-muted">
            {selected?.url}
          </div>
          <div className="text-[11px] text-muted">
            选择账号后会使用全局选择器尝试填充；没有配置时建议只打开。
          </div>
          <div className="flex flex-wrap gap-1.5">
            <button
              type="button"
              onClick={() => setAccountId("")}
              className={`rounded-full border px-2.5 py-1 text-[11px] ${
                accountId === ""
                  ? "border-accent/60 bg-accent/12 text-accent"
                  : "border-line bg-elev text-muted"
              }`}
            >
              不填密码
            </button>
            {config.accounts.map((account) => (
              <button
                key={account.id}
                type="button"
                onClick={() => setAccountId(account.id)}
                className={`rounded-full border px-2.5 py-1 text-[11px] ${
                  accountId === account.id
                    ? "border-accent/60 bg-accent/12 text-accent"
                    : "border-line bg-elev text-muted"
                }`}
              >
                {account.label}
                <span className="ml-1 text-[9px] opacity-60">
                  {groupLabel(account.group)}
                </span>
              </button>
            ))}
          </div>
        </div>
      </Dialog>
    </div>
  );
}
