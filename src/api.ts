import { invoke } from "@tauri-apps/api/core";
import type { Account, Overview, Session } from "./types";

const demoOverview: Overview = {
  accounts: [
    { id: "account-a", name: "Primary", codexHome: "~/.cx/accounts/account-a", status: "ready", remainingPercent: 36, activeSessions: 2 },
    { id: "account-b", name: "Backup", codexHome: "~/.cx/accounts/account-b", status: "ready", remainingPercent: 82, activeSessions: 1 },
    { id: "account-c", name: "Reserve", codexHome: "~/.cx/accounts/account-c", status: "exhausted", remainingPercent: 0, activeSessions: 0, cooldownUntil: new Date(Date.now() + 42 * 60 * 1000).toISOString() },
  ],
  sessions: [
    { id: "s1", name: "backend-refactor", projectPath: "~/Projects/backend", threadId: "019c8f4a…", pid: 41882, runtime: "spine-codex", status: "running", accountId: "account-a", startedAt: new Date(Date.now() - 2.4 * 3600_000).toISOString(), lastActivityAt: new Date(Date.now() - 18_000).toISOString(), lastMessage: "Running integration tests…", managed: true, autoFailover: true },
    { id: "s2", name: "ios-migration", projectPath: "~/Projects/mobile", threadId: "019c91b2…", pid: 42119, runtime: "spine-codex", status: "running", accountId: "account-b", startedAt: new Date(Date.now() - 49 * 60_000).toISOString(), lastActivityAt: new Date(Date.now() - 8_000).toISOString(), lastMessage: "Editing networking layer", managed: true, autoFailover: true },
    { id: "s3", name: "api-tests", projectPath: "~/Projects/api", runtime: "codex", status: "paused", accountId: "account-a", startedAt: new Date(Date.now() - 35 * 60_000).toISOString(), lastActivityAt: new Date(Date.now() - 12 * 60_000).toISOString(), lastMessage: "Paused before regression suite", managed: true, autoFailover: false },
  ],
};

function isTauri() {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export async function getOverview(): Promise<Overview> {
  if (!isTauri()) return demoOverview;
  return invoke<Overview>("get_overview");
}

export async function scanExisting(): Promise<Session[]> {
  if (!isTauri()) return demoOverview.sessions;
  return invoke<Session[]>("scan_existing_runtimes");
}

export async function switchAccount(sessionId: string, accountId: string): Promise<Session> {
  if (!isTauri()) {
    const session = demoOverview.sessions.find((s) => s.id === sessionId)!;
    session.accountId = accountId;
    session.status = "recovering";
    session.lastMessage = `Failover scheduled → ${accountId}`;
    setTimeout(() => { session.status = "running"; }, 700);
    return { ...session };
  }
  return invoke<Session>("switch_session_account", { sessionId, accountId });
}

export async function setAutoFailover(sessionId: string, enabled: boolean): Promise<Session> {
  if (!isTauri()) {
    const session = demoOverview.sessions.find((s) => s.id === sessionId)!;
    session.autoFailover = enabled;
    return { ...session };
  }
  return invoke<Session>("set_auto_failover", { sessionId, enabled });
}

export async function addAccount(name: string, codexHome: string): Promise<Account> {
  if (!isTauri()) {
    const account: Account = { id: `account-${Date.now()}`, name, codexHome, status: "ready", activeSessions: 0 };
    demoOverview.accounts.push(account);
    return account;
  }
  return invoke<Account>("add_account", { name, codexHome });
}
