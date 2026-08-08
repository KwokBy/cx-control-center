export type SessionStatus = "running" | "recovering" | "paused" | "failed" | "completed";
export type AccountStatus = "ready" | "exhausted" | "disabled";

export interface Session {
  id: string;
  name: string;
  projectPath: string;
  threadId?: string;
  pid?: number;
  runtime: "spine-codex" | "codex";
  status: SessionStatus;
  accountId?: string;
  startedAt: string;
  lastActivityAt: string;
  lastMessage?: string;
  managed: boolean;
  autoFailover: boolean;
}

export interface Account {
  id: string;
  name: string;
  codexHome: string;
  status: AccountStatus;
  remainingPercent?: number;
  cooldownUntil?: string;
  activeSessions: number;
  sharedSessionsReady?: boolean;
}

export interface Overview {
  sessions: Session[];
  accounts: Account[];
}

export interface SupervisorTick {
  changedSessions: Session[];
  changedAccounts: Account[];
  notices: string[];
}
