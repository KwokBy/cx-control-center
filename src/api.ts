import { invoke } from "@tauri-apps/api/core";
import type { Account, Overview, Session, SupervisorTick } from "./types";

const demoOverview: Overview = {
  accounts: [
    { id: "account-a", name: "Primary", codexHome: "~/.cx/accounts/account-a", status: "ready", remainingPercent: 36, activeSessions: 2, sharedSessionsReady: true },
    { id: "account-b", name: "Backup", codexHome: "~/.cx/accounts/account-b", status: "ready", remainingPercent: 82, activeSessions: 1, sharedSessionsReady: true },
  ],
  sessions: [
    { id: "s1", name: "backend-refactor", projectPath: "~/Projects/backend", threadId: "019c8f4a-demo", pid: 41882, runtime: "spine-codex", status: "running", accountId: "account-a", startedAt: new Date(Date.now() - 2.4 * 3600_000).toISOString(), lastActivityAt: new Date(Date.now() - 18_000).toISOString(), lastMessage: "Running integration tests…", managed: true, autoFailover: true },
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
  if (!isTauri()) return [];
  return invoke<Session[]>("scan_existing_runtimes");
}

export async function attachSession(input: { pid: number; name: string; projectPath: string; threadId: string; accountId: string; runtime: string }): Promise<Session> {
  if (!isTauri()) return { ...demoOverview.sessions[0], ...input, id: `session-${Date.now()}`, managed: true, autoFailover: false } as Session;
  return invoke<Session>("attach_session", input);
}

export async function switchAccount(sessionId: string, accountId: string): Promise<Session> {
  if (!isTauri()) return { ...demoOverview.sessions[0], id: sessionId, accountId };
  return invoke<Session>("switch_session_account", { sessionId, accountId });
}

export async function setAutoFailover(sessionId: string, enabled: boolean): Promise<Session> {
  if (!isTauri()) return { ...demoOverview.sessions[0], id: sessionId, autoFailover: enabled };
  return invoke<Session>("set_auto_failover", { sessionId, enabled });
}

export async function addAccount(name: string, codexHome: string): Promise<Account> {
  if (!isTauri()) return { id: `account-${Date.now()}`, name, codexHome, status: "ready", activeSessions: 0 };
  return invoke<Account>("add_account", { name, codexHome });
}

export async function prepareAccount(accountId: string): Promise<Account> {
  if (!isTauri()) {
    const account = demoOverview.accounts.find((a) => a.id === accountId)!;
    return { ...account, sharedSessionsReady: true };
  }
  return invoke<Account>("prepare_account", { accountId });
}

export async function markAccountQuota(accountId: string, remainingPercent?: number): Promise<Account> {
  if (!isTauri()) return { ...demoOverview.accounts.find((a) => a.id === accountId)!, remainingPercent, status: remainingPercent === 0 ? "exhausted" : "ready" };
  return invoke<Account>("mark_account_quota", { accountId, remainingPercent });
}

export async function supervisorTick(): Promise<SupervisorTick> {
  if (!isTauri()) return { changedSessions: [], changedAccounts: demoOverview.accounts, notices: [] };
  return invoke<SupervisorTick>("supervisor_tick");
}
