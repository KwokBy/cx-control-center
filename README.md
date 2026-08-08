# CX Control Center

A local desktop control plane for long-running Codex / SpineCodex sessions, isolated account homes, and process-level account failover.

## Goals

- Treat the **session/task** as the durable unit of work, not the OS process.
- Keep each Codex account in its own `CODEX_HOME`.
- Discover currently running `codex` and `spine-codex` processes on macOS.
- Switch a managed session from one account to another by terminating the old runtime and resuming the captured thread under the target account.
- Provide a UI for sessions, accounts, failover policy, activity and recovery.

## Stack

- Tauri 2
- React 19 + TypeScript + Vite
- Rust supervisor
- JSON state store for the MVP (`~/.cx-control-center/state.json`); SQLite is planned once the session schema stabilizes.

## Development

Prerequisites: Node.js 20+, Rust stable, and the platform requirements for Tauri 2.

```bash
npm install
npm run tauri dev
```

Frontend only:

```bash
npm run dev
```

## Account isolation

Create one home per account, then authenticate that home independently:

```bash
mkdir -p ~/.cx/accounts/account-a
CODEX_HOME="$HOME/.cx/accounts/account-a" codex login

mkdir -p ~/.cx/accounts/account-b
CODEX_HOME="$HOME/.cx/accounts/account-b" codex login
```

Add those homes to the **Accounts** screen. CX never copies `auth.json` between account homes.

## Failover model

The MVP uses process-level recovery:

1. Mark the session `recovering`.
2. Gracefully `SIGTERM` the old process.
3. Select the target account's isolated `CODEX_HOME`.
4. Spawn `codex resume <thread-id>` or `spine-codex resume <thread-id>` in the project directory.
5. Bind the new PID/account back to the same logical CX session.

`src-tauri/src/lib.rs::switch_session_account` is intentionally the adapter seam for SpineCodex. If your local SpineCodex build uses different resume arguments, change only that runtime adapter path.

## Current MVP scope

Implemented:

- Session dashboard and detail view
- Account pool UI
- Isolated account homes
- macOS runtime discovery
- Persistent local state
- Manual Switch & Resume backend path
- Automatic failover policy flag
- Runtime/account/session separation

Next:

- Managed attachment wizard (project path, thread id, account mapping)
- Real stdout/stderr streaming for spawned runtimes
- Quota/rate-limit event classifier and automatic failover execution
- SpineCodex-specific adapter after validating its exact local CLI/session format
- SQLite event history
- macOS Keychain integration for manager-owned secrets (Codex credentials remain owned by Codex)

## Safety

CX does not merge or copy account credential files. Switching is only allowed for a managed session with a captured thread id; discovered processes are read-only until attached.
