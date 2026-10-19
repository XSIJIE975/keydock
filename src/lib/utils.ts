import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

export function errorMessage(error: unknown): string {
  if (typeof error === "string") return error;
  if (error && typeof error === "object") {
    const candidate = error as { message?: unknown; code?: unknown };
    if (typeof candidate.message === "string") return candidate.message;
  }
  return "操作失败，请重试";
}

export function errorCode(error: unknown): string | null {
  if (error && typeof error === "object") {
    const candidate = error as { code?: unknown };
    if (typeof candidate.code === "string") return candidate.code;
  }
  return null;
}

export function shortHost(url: string): string {
  try {
    return new URL(url).host;
  } catch {
    return url.replace(/^https?:\/\//, "").split("/")[0];
  }
}

export function groupLabel(group: string): string {
  return group.trim() ? group : "未分组";
}

export function accountMatches(account: { group: string }, siteGroup: string): boolean {
  return !account.group || !siteGroup || account.group === siteGroup;
}
