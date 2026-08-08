import { useEffect, useMemo, useState } from "react";
import { Activity, CirclePause, Cpu, Gauge, GitBranch, Plus, RefreshCw, RotateCw, Server, Settings, ShieldCheck, TerminalSquare, Zap } from "lucide-react";
import { addAccount, getOverview, scanExisting, setAutoFailover, switchAccount } from "./api";
import type { Account, Overview, Session } from "./types";

const statusLabel: Record<Session["status"], string> = {
  running: "Running",
  recovering: "Recovering",
  paused: "Paused",
  failed: "Failed",
  completed: "Completed",
};

function timeAgo(iso: string) {
  const diff = Math.max(0, Date.now() - new Date(iso).getTime());
  if (diff < 60_000) return `${Math.floor(diff / 1000)}s ago`;
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)}m ago`;
  return `${Math.floor(diff / 3_600_000)}h ago`;
}

function StatusDot({ status }: { status: Session["status"] }) {
  return <span className={`status-dot ${status}`} />;
}

function Sidebar({ active, onChange }: { active: string; onChange: (v: string) => void }) {
  return (
    <aside className="sidebar">
      <div className="brand"><div className="brand-mark">CX</div><div><strong>Control Center</strong><small>Codex orchestration</small></div></div>
      <nav>
        <button className={active === "sessions" ? "active" : ""} onClick={() => onChange("sessions")}><Activity size={18} />Sessions</button>
        <button className={active === "accounts" ? "active" : ""} onClick={() => onChange("accounts")}><Server size={18} />Accounts</button>
        <button className={active === "settings" ? "active" : ""} onClick={() => onChange("settings")}><Settings size={18} />Settings</button>
      </nav>
      <div className="sidebar-footer"><span className="pulse" /> supervisor ready</div>
    </aside>
  );
}

function AccountBadge({ account }: { account?: Account }) {
  if (!account) return <span className="chip muted">unassigned</span>;
  return <span className={`chip ${account.status}`}>{account.name}</span>;
}

function SessionList({ sessions, selected, onSelect, accounts }: { sessions: Session[]; selected?: string; onSelect: (id: string) => void; accounts: Account[] }) {
  return <div className="session-list">
    {sessions.map((session) => {
      const account = accounts.find((a) => a.id === session.accountId);
      return <button key={session.id} className={`session-row ${selected === session.id ? "selected" : ""}`} onClick={() => onSelect(session.id)}>
        <div className="session-title"><StatusDot status={session.status} /><strong>{session.name}</strong><span>{timeAgo(session.lastActivityAt)}</span></div>
        <div className="session-meta"><AccountBadge account={account} /><span>{session.runtime}</span>{session.pid && <span>PID {session.pid}</span>}</div>
        <p>{session.lastMessage || "No recent activity"}</p>
      </button>;
    })}
  </div>;
}

function SessionDetail({ session, accounts, onChanged }: { session: Session; accounts: Account[]; onChanged: (s: Session) => void }) {
  const current = accounts.find((a) => a.id === session.accountId);
  const [switching, setSwitching] = useState(false);
  const [target, setTarget] = useState(accounts.find((a) => a.status === "ready" && a.id !== session.accountId)?.id || "");

  async function doSwitch() {
    if (!target) return;
    setSwitching(true);
    try { onChanged(await switchAccount(session.id, target)); } finally { setSwitching(false); }
  }

  async function toggleAuto() {
    onChanged(await setAutoFailover(session.id, !session.autoFailover));
  }

  return <section className="detail-card">
    <div className="detail-header">
      <div><div className="eyebrow">SESSION</div><h1>{session.name}</h1><div className="subtitle"><StatusDot status={session.status} />{statusLabel[session.status]} · {session.runtime}</div></div>
      <div className="actions"><button className="secondary"><TerminalSquare size={16}/>Open Terminal</button><button className="secondary"><CirclePause size={16}/>Pause</button></div>
    </div>

    <div className="metric-grid">
      <div className="metric"><Cpu size={17}/><span>Runtime</span><strong>{session.pid ? `PID ${session.pid}` : "Detached"}</strong></div>
      <div className="metric"><GitBranch size={17}/><span>Thread</span><strong>{session.threadId || "Not captured"}</strong></div>
      <div className="metric"><Gauge size={17}/><span>Account quota</span><strong>{current?.remainingPercent == null ? "Unknown" : `${current.remainingPercent}%`}</strong></div>
      <div className="metric"><ShieldCheck size={17}/><span>Failover</span><strong>{session.autoFailover ? "Automatic" : "Manual"}</strong></div>
    </div>

    <div className="path-box"><span>Project</span><code>{session.projectPath}</code></div>

    <div className="failover-panel">
      <div className="panel-copy"><div className="eyebrow">ACCOUNT FAILOVER</div><h3>Switch & Resume</h3><p>The session remains the unit of work. CX restarts the runtime under another isolated CODEX_HOME and resumes the captured thread.</p></div>
      <div className="switch-controls">
        <div className="current-account"><small>Current account</small><AccountBadge account={current} /></div>
        <span className="arrow">→</span>
        <select value={target} onChange={(e) => setTarget(e.target.value)}>
          <option value="">Select target</option>
          {accounts.filter((a) => a.id !== session.accountId && a.status === "ready").map((a) => <option key={a.id} value={a.id}>{a.name} · {a.remainingPercent ?? "?"}%</option>)}
        </select>
        <button className="primary" disabled={!target || switching} onClick={doSwitch}><RotateCw size={16} className={switching ? "spin" : ""}/>{switching ? "Switching" : "Switch & Resume"}</button>
      </div>
      <label className="toggle-row"><span><strong>Automatic quota failover</strong><small>When a quota/rate-limit failure is detected, pick the best ready account and resume this session.</small></span><input type="checkbox" checked={session.autoFailover} onChange={toggleAuto}/></label>
    </div>

    <div className="live-log"><div className="log-head"><span>LIVE ACTIVITY</span><span className="live"><i/>streaming</span></div><pre>{`11:31  attached runtime ${session.pid || "—"}\n11:32  ${session.lastMessage || "waiting for activity"}\n11:34  supervisor heartbeat ok\n11:35  failover policy: ${session.autoFailover ? "automatic" : "manual"}`}</pre></div>
  </section>;
}

function AccountsPage({ accounts, onAdd }: { accounts: Account[]; onAdd: (a: Account) => void }) {
  const [show, setShow] = useState(false);
  const [name, setName] = useState("");
  const [home, setHome] = useState("~/.cx/accounts/");
  return <div className="page-card">
    <div className="page-title"><div><div className="eyebrow">ACCOUNT POOL</div><h1>Accounts</h1><p>Each account owns an isolated CODEX_HOME. Credentials are never copied between accounts.</p></div><button className="primary" onClick={() => setShow(!show)}><Plus size={16}/>Add account</button></div>
    {show && <div className="add-form"><input placeholder="Display name" value={name} onChange={(e)=>setName(e.target.value)}/><input placeholder="CODEX_HOME" value={home} onChange={(e)=>setHome(e.target.value)}/><button className="primary" onClick={async()=>{ if(name && home){onAdd(await addAccount(name, home)); setShow(false); setName("");}}}>Save</button></div>}
    <div className="account-table"><div className="account-tr header"><span>Name</span><span>Status</span><span>Quota</span><span>Active</span><span>CODEX_HOME</span></div>{accounts.map(a=><div className="account-tr" key={a.id}><strong>{a.name}</strong><span className={`chip ${a.status}`}>{a.status}</span><span>{a.remainingPercent == null ? "—" : `${a.remainingPercent}%`}</span><span>{a.activeSessions}</span><code>{a.codexHome}</code></div>)}</div>
  </div>;
}

function SettingsPage() {
  return <div className="page-card"><div className="page-title"><div><div className="eyebrow">POLICY</div><h1>Settings</h1><p>Defaults for runtime recovery and account scheduling.</p></div></div><div className="settings-grid"><div><label>Failover strategy</label><select defaultValue="quota"><option value="quota">Highest remaining quota</option><option value="least">Least active sessions</option><option value="round">Round robin</option></select></div><div><label>Default runtime</label><select defaultValue="spine"><option value="spine">spine-codex</option><option value="codex">codex</option></select></div><div className="wide"><label>Recovery prompt</label><textarea defaultValue="Continue the interrupted task from where it stopped. Inspect the current working tree and previous session context. Do not redo completed work. Continue until the original task is complete and verify the result."/></div></div></div>;
}

export default function App() {
  const [overview, setOverview] = useState<Overview>({sessions: [], accounts: []});
  const [active, setActive] = useState("sessions");
  const [selected, setSelected] = useState<string>();
  const [loading, setLoading] = useState(true);

  async function refresh() {
    setLoading(true); const data = await getOverview(); setOverview(data); setSelected((s) => s || data.sessions[0]?.id); setLoading(false);
  }
  useEffect(() => { refresh(); }, []);
  const session = useMemo(() => overview.sessions.find((s) => s.id === selected), [overview.sessions, selected]);
  const replaceSession = (next: Session) => setOverview((o) => ({...o, sessions: o.sessions.map((s)=>s.id===next.id?next:s)}));

  async function discover() {
    const found = await scanExisting();
    setOverview((o) => ({...o, sessions: [...o.sessions, ...found.filter(f=>!o.sessions.some(s=>s.pid && s.pid===f.pid))]}));
  }

  return <div className="app-shell"><Sidebar active={active} onChange={setActive}/><main>
    <header className="topbar"><div><span className="workspace">LOCAL WORKSPACE</span><span className="health"><i/>Supervisor online</span></div><div className="top-actions"><button className="secondary" onClick={discover}><Zap size={15}/>Discover runtimes</button><button className="icon-btn" onClick={refresh}><RefreshCw size={17} className={loading ? "spin" : ""}/></button></div></header>
    {active === "sessions" && <div className="sessions-layout"><div className="left-pane"><div className="pane-head"><div><h2>Sessions</h2><span>{overview.sessions.filter(s=>s.status==="running").length} active</span></div></div><SessionList sessions={overview.sessions} selected={selected} onSelect={setSelected} accounts={overview.accounts}/></div><div className="content-pane">{session ? <SessionDetail session={session} accounts={overview.accounts} onChanged={replaceSession}/> : <div className="empty">No session selected</div>}</div></div>}
    {active === "accounts" && <AccountsPage accounts={overview.accounts} onAdd={(a)=>setOverview(o=>({...o, accounts:[...o.accounts,a]}))}/>} 
    {active === "settings" && <SettingsPage/>}
  </main></div>;
}
