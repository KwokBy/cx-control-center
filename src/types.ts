export type SessionStatus = "running" | "recovering" | "paused" | "failed" | "completed";
export type AccountStatus = "ready" | "exhausted" | "disabled";

export interface ThreadLineageEntry {
  threadId: string;
  parentThreadId?: string;
  accountId?: string;
  rolloutPath?: string;
  discoveredAt: string;
}

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
  threadLineage: ThreadLineageEntry[];
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

export interface FailoverEvent {
  id: string;
  sessionId: string;
  eventType: "attach" | "failover" | "failover_failed" | "thread_lineage" | string;
  at: string;
  fromAccountId?: string;
  toAccountId?: string;
  fromThreadId?: string;
  toThreadId?: string;
  message: string;
  automatic: boolean;
}

export interface ThreadCandidate {
  threadId: string;
  parentThreadId?: string;
  cwd?: string;
  rolloutPath: string;
  modifiedAt: number;
  accountId?: string;
  confidence: number;
  reasons: string[];
}

export interface Overview {
  sessions: Session[];
  accounts: Account[];
  events: FailoverEvent[];
}

export interface SupervisorTick {
  changedSessions: Session[];
  changedAccounts: Account[];
  notices: string[];
  newEvents: FailoverEvent[];
}
