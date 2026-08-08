use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf, process::Command, sync::Mutex};
use tauri::State;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Session {
    id: String,
    name: String,
    project_path: String,
    thread_id: Option<String>,
    pid: Option<u32>,
    runtime: String,
    status: String,
    account_id: Option<String>,
    started_at: String,
    last_activity_at: String,
    last_message: Option<String>,
    managed: bool,
    auto_failover: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Account {
    id: String,
    name: String,
    codex_home: String,
    status: String,
    remaining_percent: Option<u8>,
    cooldown_until: Option<String>,
    active_sessions: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct Overview {
    sessions: Vec<Session>,
    accounts: Vec<Account>,
}

struct AppState(Mutex<Overview>);

fn store_path() -> PathBuf {
    let base = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join(".cx-control-center").join("state.json")
}

fn load_state() -> Overview {
    let path = store_path();
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn persist(state: &Overview) -> Result<(), String> {
    let path = store_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(state).map_err(|e| e.to_string())?;
    fs::write(path, json).map_err(|e| e.to_string())
}

fn now_iso() -> String {
    format!("unix:{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs())
}

#[tauri::command]
fn get_overview(state: State<'_, AppState>) -> Result<Overview, String> {
    state.0.lock().map(|s| s.clone()).map_err(|e| e.to_string())
}

#[tauri::command]
fn add_account(name: String, codex_home: String, state: State<'_, AppState>) -> Result<Account, String> {
    let account = Account {
        id: format!("account-{}", Uuid::new_v4()),
        name,
        codex_home,
        status: "ready".into(),
        remaining_percent: None,
        cooldown_until: None,
        active_sessions: 0,
    };
    let mut data = state.0.lock().map_err(|e| e.to_string())?;
    data.accounts.push(account.clone());
    persist(&data)?;
    Ok(account)
}

#[tauri::command]
fn set_auto_failover(session_id: String, enabled: bool, state: State<'_, AppState>) -> Result<Session, String> {
    let mut data = state.0.lock().map_err(|e| e.to_string())?;
    let next = {
        let session = data.sessions.iter_mut().find(|s| s.id == session_id).ok_or("session not found")?;
        session.auto_failover = enabled;
        session.clone()
    };
    persist(&data)?;
    Ok(next)
}

#[tauri::command]
fn scan_existing_runtimes() -> Result<Vec<Session>, String> {
    let output = Command::new("ps")
        .args(["-axo", "pid=,etime=,command="])
        .output()
        .map_err(|e| format!("failed to run ps: {e}"))?;
    let text = String::from_utf8_lossy(&output.stdout);
    let mut found = Vec::new();
    for line in text.lines() {
        let lower = line.to_lowercase();
        if !(lower.contains("spine-codex") || lower.contains("codex")) || lower.contains("cx-control-center") {
            continue;
        }
        let mut parts = line.trim().split_whitespace();
        let Some(pid_text) = parts.next() else { continue };
        let Ok(pid) = pid_text.parse::<u32>() else { continue };
        let _elapsed = parts.next().unwrap_or("");
        let command = parts.collect::<Vec<_>>().join(" ");
        let runtime = if lower.contains("spine-codex") { "spine-codex" } else { "codex" };
        found.push(Session {
            id: format!("attached-{pid}"),
            name: format!("{} · {}", runtime, pid),
            project_path: "unknown — attach to enrich".into(),
            thread_id: None,
            pid: Some(pid),
            runtime: runtime.into(),
            status: "running".into(),
            account_id: None,
            started_at: now_iso(),
            last_activity_at: now_iso(),
            last_message: Some(command),
            managed: false,
            auto_failover: false,
        });
    }
    Ok(found)
}

fn account_home(data: &Overview, account_id: &str) -> Result<String, String> {
    data.accounts.iter().find(|a| a.id == account_id)
        .map(|a| a.codex_home.clone())
        .ok_or_else(|| "target account not found".to_string())
}

#[tauri::command]
fn switch_session_account(session_id: String, account_id: String, state: State<'_, AppState>) -> Result<Session, String> {
    let mut data = state.0.lock().map_err(|e| e.to_string())?;
    let home = account_home(&data, &account_id)?;
    let idx = data.sessions.iter().position(|s| s.id == session_id).ok_or("session not found")?;
    let snapshot = data.sessions[idx].clone();
    if !snapshot.managed {
        return Err("attach this runtime as a managed session before switching accounts".into());
    }
    let thread = snapshot.thread_id.clone().ok_or("session has no captured thread id; cannot resume safely")?;

    data.sessions[idx].status = "recovering".into();
    data.sessions[idx].last_message = Some(format!("switching to {account_id}"));
    persist(&data)?;

    if let Some(pid) = snapshot.pid {
        let _ = Command::new("kill").args(["-TERM", &pid.to_string()]).status();
    }

    let executable = if snapshot.runtime == "spine-codex" { "spine-codex" } else { "codex" };
    let mut cmd = Command::new(executable);
    cmd.arg("resume").arg(&thread).env("CODEX_HOME", &home).current_dir(&snapshot.project_path);
    let child = cmd.spawn().map_err(|e| format!("failed to resume {executable}: {e}"))?;

    data.sessions[idx].pid = Some(child.id());
    data.sessions[idx].account_id = Some(account_id);
    data.sessions[idx].status = "running".into();
    data.sessions[idx].last_activity_at = now_iso();
    data.sessions[idx].last_message = Some(format!("resumed thread {thread}"));
    let result = data.sessions[idx].clone();
    persist(&data)?;
    Ok(result)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState(Mutex::new(load_state())))
        .invoke_handler(tauri::generate_handler![
            get_overview,
            add_account,
            set_auto_failover,
            scan_existing_runtimes,
            switch_session_account,
        ])
        .run(tauri::generate_context!())
        .expect("error while running CX Control Center");
}
