use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashMap,
    fs,
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::Command,
    sync::Mutex,
    time::UNIX_EPOCH,
};
use tauri::State;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadLineageEntry {
    thread_id: String,
    parent_thread_id: Option<String>,
    account_id: Option<String>,
    rollout_path: Option<String>,
    discovered_at: String,
}

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
    #[serde(default)]
    thread_lineage: Vec<ThreadLineageEntry>,
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
    #[serde(default)]
    shared_sessions_ready: bool,
    #[serde(default)]
    log_cursor: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FailoverEvent {
    id: String,
    session_id: String,
    event_type: String,
    at: String,
    from_account_id: Option<String>,
    to_account_id: Option<String>,
    from_thread_id: Option<String>,
    to_thread_id: Option<String>,
    message: String,
    automatic: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadCandidate {
    thread_id: String,
    parent_thread_id: Option<String>,
    cwd: Option<String>,
    rollout_path: String,
    modified_at: u64,
    account_id: Option<String>,
    confidence: u8,
    reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct Overview {
    sessions: Vec<Session>,
    accounts: Vec<Account>,
    #[serde(default)]
    events: Vec<FailoverEvent>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SupervisorTick {
    changed_sessions: Vec<Session>,
    changed_accounts: Vec<Account>,
    notices: Vec<String>,
    new_events: Vec<FailoverEvent>,
}

struct AppState(Mutex<Overview>);

fn cx_home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")).join(".cx-control-center")
}
fn store_path() -> PathBuf { cx_home().join("state.json") }
fn shared_sessions_path() -> PathBuf { cx_home().join("shared-codex").join("sessions") }
fn expand_home(value: &str) -> PathBuf {
    if value == "~" { return dirs::home_dir().unwrap_or_else(|| PathBuf::from(value)); }
    if let Some(rest) = value.strip_prefix("~/") {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")).join(rest);
    }
    PathBuf::from(value)
}
fn load_state() -> Overview {
    fs::read_to_string(store_path()).ok().and_then(|raw| serde_json::from_str(&raw).ok()).unwrap_or_default()
}
fn persist(state: &Overview) -> Result<(), String> {
    let path = store_path();
    if let Some(parent) = path.parent() { fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
    fs::write(path, serde_json::to_string_pretty(state).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn now_iso() -> String {
    format!("unix:{}", std::time::SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs())
}
fn process_cwd(pid: u32) -> Option<String> {
    let output = Command::new("lsof").args(["-a", "-p", &pid.to_string(), "-d", "cwd", "-Fn"]).output().ok()?;
    String::from_utf8_lossy(&output.stdout).lines().find_map(|line| line.strip_prefix('n').map(ToOwned::to_owned))
}
fn process_command(pid: u32) -> Option<String> {
    let output = Command::new("ps").args(["-p", &pid.to_string(), "-o", "command="]).output().ok()?;
    let command = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!command.is_empty()).then_some(command)
}
fn recompute_active_sessions(data: &mut Overview) {
    let counts = data.sessions.iter().filter(|s| s.status == "running" || s.status == "recovering")
        .filter_map(|s| s.account_id.as_ref()).fold(HashMap::<String,u32>::new(), |mut acc,id| { *acc.entry(id.clone()).or_default() += 1; acc });
    for account in &mut data.accounts { account.active_sessions = counts.get(&account.id).copied().unwrap_or(0); }
}
fn account_home(data: &Overview, account_id: &str) -> Result<PathBuf,String> {
    data.accounts.iter().find(|a| a.id == account_id).map(|a| expand_home(&a.codex_home)).ok_or_else(|| "target account not found".to_string())
}
fn choose_failover_account(data: &Overview, current: &str) -> Option<String> {
    let mut candidates = data.accounts.iter().filter(|a| a.id != current && a.status == "ready" && a.shared_sessions_ready).collect::<Vec<_>>();
    candidates.sort_by(|a,b| b.remaining_percent.unwrap_or(50).cmp(&a.remaining_percent.unwrap_or(50)).then(a.active_sessions.cmp(&b.active_sessions)));
    candidates.first().map(|a| a.id.clone())
}
fn copy_dir_recursive(from: &Path, to: &Path) -> Result<(),String> {
    fs::create_dir_all(to).map_err(|e| e.to_string())?;
    for entry in fs::read_dir(from).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?; let src = entry.path(); let dst = to.join(entry.file_name()); let ty = entry.file_type().map_err(|e| e.to_string())?;
        if ty.is_dir() { copy_dir_recursive(&src,&dst)?; } else if ty.is_file() && !dst.exists() { fs::copy(&src,&dst).map_err(|e| e.to_string())?; }
    }
    Ok(())
}
fn collect_jsonl(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else { return; };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() { collect_jsonl(&p,out); }
        else if p.extension().and_then(|x| x.to_str()) == Some("jsonl") && p.file_name().and_then(|x| x.to_str()).map(|x| x.starts_with("rollout-")).unwrap_or(false) { out.push(p); }
    }
}
fn string_at<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|k| value.get(*k).and_then(|v| v.as_str()))
}
fn parse_rollout_meta(path: &Path) -> Option<(String,Option<String>,Option<String>)> {
    let file = fs::File::open(path).ok()?;
    for line in BufReader::new(file).lines().take(12).flatten() {
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue; };
        let payload = v.get("payload").unwrap_or(&v);
        let ty = v.get("type").and_then(|x| x.as_str()).or_else(|| payload.get("type").and_then(|x| x.as_str()));
        if ty != Some("session_meta") && payload.get("cwd").is_none() { continue; }
        let thread = string_at(payload,&["id","session_id","thread_id"])
            .map(ToOwned::to_owned)
            .or_else(|| path.file_stem().and_then(|x| x.to_str()).and_then(|s| s.rsplit('-').next()).map(ToOwned::to_owned))?;
        let cwd = string_at(payload,&["cwd"]).map(ToOwned::to_owned);
        let parent = string_at(payload,&["forked_from_id","forkedFromId"]).map(ToOwned::to_owned);
        return Some((thread,cwd,parent));
    }
    None
}
fn thread_candidates_for(home: &Path, account_id: Option<&str>, cwd: Option<&str>) -> Vec<ThreadCandidate> {
    let mut paths = Vec::new(); collect_jsonl(&home.join("sessions"), &mut paths);
    let mut result = Vec::new();
    for path in paths {
        let Some((thread_id,meta_cwd,parent_thread_id)) = parse_rollout_meta(&path) else { continue; };
        let modified_at = fs::metadata(&path).ok().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
        let mut score = 20u8; let mut reasons = vec!["valid Codex rollout metadata".to_string()];
        if let (Some(expected),Some(actual)) = (cwd,meta_cwd.as_deref()) {
            if Path::new(expected) == Path::new(actual) { score = score.saturating_add(60); reasons.push("cwd exact match".into()); }
            else if actual.starts_with(expected) || expected.starts_with(actual) { score = score.saturating_add(25); reasons.push("cwd related".into()); }
        }
        let age = std::time::SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs().saturating_sub(modified_at);
        if age < 300 { score = score.saturating_add(20); reasons.push("modified in last 5 minutes".into()); }
        else if age < 3600 { score = score.saturating_add(10); reasons.push("modified in last hour".into()); }
        result.push(ThreadCandidate { thread_id,parent_thread_id,cwd:meta_cwd,rollout_path:path.to_string_lossy().into(),modified_at,account_id:account_id.map(ToOwned::to_owned),confidence:score.min(100),reasons });
    }
    result.sort_by(|a,b| b.confidence.cmp(&a.confidence).then(b.modified_at.cmp(&a.modified_at)));
    result
}
fn append_event(data: &mut Overview, session_id: &str, event_type: &str, from_account_id: Option<String>, to_account_id: Option<String>, from_thread_id: Option<String>, to_thread_id: Option<String>, message: String, automatic: bool) -> FailoverEvent {
    let ev = FailoverEvent { id: format!("event-{}",Uuid::new_v4()),session_id:session_id.to_string(),event_type:event_type.into(),at:now_iso(),from_account_id,to_account_id,from_thread_id,to_thread_id,message,automatic };
    data.events.push(ev.clone()); if data.events.len() > 2000 { data.events.drain(0..data.events.len()-2000); } ev
}
fn refresh_lineage(data: &mut Overview, session_id: &str) -> Option<(Session,FailoverEvent)> {
    let idx = data.sessions.iter().position(|s| s.id == session_id)?; let snap = data.sessions[idx].clone();
    let current = snap.thread_id.as_deref()?; let account_id = snap.account_id.as_deref()?; let home = account_home(data,account_id).ok()?;
    let candidate = thread_candidates_for(&home,Some(account_id),Some(&snap.project_path)).into_iter()
        .find(|c| c.thread_id != current && c.parent_thread_id.as_deref() == Some(current) && c.confidence >= 70)?;
    let entry = ThreadLineageEntry { thread_id:candidate.thread_id.clone(),parent_thread_id:candidate.parent_thread_id.clone(),account_id:Some(account_id.to_string()),rollout_path:Some(candidate.rollout_path.clone()),discovered_at:now_iso() };
    if data.sessions[idx].thread_lineage.iter().any(|x| x.thread_id == entry.thread_id) { return None; }
    let old = data.sessions[idx].thread_id.clone(); data.sessions[idx].thread_id = Some(entry.thread_id.clone()); data.sessions[idx].thread_lineage.push(entry.clone()); data.sessions[idx].last_message = Some(format!("thread lineage advanced → {}",entry.thread_id)); data.sessions[idx].last_activity_at = now_iso();
    let session = data.sessions[idx].clone();
    let event = append_event(data,session_id,"thread_lineage",Some(account_id.to_string()),Some(account_id.to_string()),old,Some(entry.thread_id),"Codex created a descendant thread during resume/fork; CX adopted it".into(),true);
    Some((session,event))
}

#[cfg(unix)]
fn prepare_shared_sessions(home: &Path) -> Result<(),String> {
    use std::os::unix::fs::symlink; fs::create_dir_all(home).map_err(|e| e.to_string())?; let shared=shared_sessions_path(); fs::create_dir_all(&shared).map_err(|e| e.to_string())?; let sessions=home.join("sessions");
    if sessions.is_symlink(){return Ok(());} if sessions.exists(){copy_dir_recursive(&sessions,&shared)?;let backup=home.join(format!("sessions.cx-backup-{}",now_iso().replace(':',"-")));fs::rename(&sessions,backup).map_err(|e| e.to_string())?;} symlink(&shared,&sessions).map_err(|e| e.to_string())?; Ok(())
}
#[cfg(not(unix))]
fn prepare_shared_sessions(_home:&Path)->Result<(),String>{Err("shared session store is currently supported on macOS/Linux only".into())}
fn quota_signal(text:&str)->bool{let lower=text.to_lowercase();["usage limit","quota exceeded","rate limit exceeded","you've hit your usage limit","you have hit your usage limit","limit reached","insufficient_quota"].iter().any(|n|lower.contains(n))}
fn account_log_path(home:&Path)->Option<PathBuf>{[home.join("log/codex-tui.log"),home.join("logs/codex-tui.log"),home.join("log/codex.log")].into_iter().find(|p|p.exists())}
fn read_log_delta(path:&Path,cursor:u64)->Result<(String,u64),String>{let mut file=fs::File::open(path).map_err(|e|e.to_string())?;let len=file.metadata().map_err(|e|e.to_string())?.len();let start=cursor.min(len);file.seek(SeekFrom::Start(start)).map_err(|e|e.to_string())?;let mut text=String::new();file.take(256*1024).read_to_string(&mut text).map_err(|e|e.to_string())?;Ok((text,len))}

fn resume_session(data:&mut Overview,session_id:&str,account_id:&str,automatic:bool)->Result<(Session,FailoverEvent),String>{
    let home=account_home(data,account_id)?;let idx=data.sessions.iter().position(|s|s.id==session_id).ok_or("session not found")?;let snapshot=data.sessions[idx].clone();if !snapshot.managed{return Err("attach this runtime as a managed session before switching accounts".into());}let thread=snapshot.thread_id.clone().ok_or("session has no captured thread id; cannot resume safely")?;
    data.sessions[idx].status="recovering".into();data.sessions[idx].last_message=Some(format!("switching to {account_id}"));
    if let Some(pid)=snapshot.pid{let _=Command::new("kill").args(["-TERM",&pid.to_string()]).status();std::thread::sleep(std::time::Duration::from_millis(350));}
    if let Some(source)=snapshot.account_id.as_deref(){if source!=account_id{if let Ok(source_home)=account_home(data,source){let source_sessions=source_home.join("sessions");if source_sessions.exists()&&!source_sessions.is_symlink(){copy_dir_recursive(&source_sessions,&shared_sessions_path())?;}}}}
    let executable=if snapshot.runtime=="spine-codex"{"spine-codex"}else{"codex"};let child=Command::new(executable).arg("resume").arg(&thread).env("CODEX_HOME",&home).current_dir(&snapshot.project_path).spawn().map_err(|e|format!("failed to resume {executable}: {e}"))?;
    data.sessions[idx].pid=Some(child.id());data.sessions[idx].account_id=Some(account_id.to_string());data.sessions[idx].status="running".into();data.sessions[idx].last_activity_at=now_iso();data.sessions[idx].last_message=Some(format!("resumed thread {thread}"));
    if data.sessions[idx].thread_lineage.iter().all(|x|x.thread_id!=thread){data.sessions[idx].thread_lineage.push(ThreadLineageEntry{thread_id:thread.clone(),parent_thread_id:None,account_id:snapshot.account_id.clone(),rollout_path:None,discovered_at:now_iso()});}
    recompute_active_sessions(data);let session=data.sessions[idx].clone();let event=append_event(data,session_id,"failover",snapshot.account_id.clone(),Some(account_id.to_string()),Some(thread.clone()),Some(thread.clone()),format!("resumed {thread} under {account_id}"),automatic);Ok((session,event))
}

#[tauri::command] fn get_overview(state:State<'_,AppState>)->Result<Overview,String>{state.0.lock().map(|s|s.clone()).map_err(|e|e.to_string())}
#[tauri::command] fn add_account(name:String,codex_home:String,state:State<'_,AppState>)->Result<Account,String>{let account=Account{id:format!("account-{}",Uuid::new_v4()),name,codex_home,status:"ready".into(),remaining_percent:None,cooldown_until:None,active_sessions:0,shared_sessions_ready:false,log_cursor:0};let mut data=state.0.lock().map_err(|e|e.to_string())?;data.accounts.push(account.clone());persist(&data)?;Ok(account)}
#[tauri::command] fn prepare_account(account_id:String,state:State<'_,AppState>)->Result<Account,String>{let mut data=state.0.lock().map_err(|e|e.to_string())?;let idx=data.accounts.iter().position(|a|a.id==account_id).ok_or("account not found")?;let home=expand_home(&data.accounts[idx].codex_home);prepare_shared_sessions(&home)?;data.accounts[idx].shared_sessions_ready=true;let result=data.accounts[idx].clone();persist(&data)?;Ok(result)}
#[tauri::command] fn set_auto_failover(session_id:String,enabled:bool,state:State<'_,AppState>)->Result<Session,String>{let mut data=state.0.lock().map_err(|e|e.to_string())?;let session=data.sessions.iter_mut().find(|s|s.id==session_id).ok_or("session not found")?;session.auto_failover=enabled;let result=session.clone();persist(&data)?;Ok(result)}
#[tauri::command] fn scan_existing_runtimes()->Result<Vec<Session>,String>{let output=Command::new("ps").args(["-axo","pid=,etime=,command="]).output().map_err(|e|format!("failed to run ps: {e}"))?;let text=String::from_utf8_lossy(&output.stdout);let mut found=Vec::new();for line in text.lines(){let lower=line.to_lowercase();if !(lower.contains("spine-codex")||lower.contains("codex"))||lower.contains("cx-control-center"){continue;}let mut parts=line.trim().split_whitespace();let Some(pid_text)=parts.next()else{continue};let Ok(pid)=pid_text.parse::<u32>()else{continue};let _=parts.next();let command=parts.collect::<Vec<_>>().join(" ");let runtime=if lower.contains("spine-codex"){"spine-codex"}else{"codex"};found.push(Session{id:format!("discovered-{pid}"),name:format!("{} · {}",runtime,pid),project_path:process_cwd(pid).unwrap_or_else(||"unknown".into()),thread_id:None,pid:Some(pid),runtime:runtime.into(),status:"running".into(),account_id:None,started_at:now_iso(),last_activity_at:now_iso(),last_message:Some(process_command(pid).unwrap_or(command)),managed:false,auto_failover:false,thread_lineage:vec![]});}Ok(found)}
#[tauri::command] fn discover_threads(project_path:String,account_id:Option<String>,state:State<'_,AppState>)->Result<Vec<ThreadCandidate>,String>{let data=state.0.lock().map_err(|e|e.to_string())?;let mut all=Vec::new();if let Some(id)=account_id{let home=account_home(&data,&id)?;all.extend(thread_candidates_for(&home,Some(&id),Some(&project_path)));}else{for a in &data.accounts{all.extend(thread_candidates_for(&expand_home(&a.codex_home),Some(&a.id),Some(&project_path)));}}all.sort_by(|a,b|b.confidence.cmp(&a.confidence).then(b.modified_at.cmp(&a.modified_at)));all.truncate(12);Ok(all)}
#[tauri::command] fn attach_session(pid:u32,name:String,project_path:String,thread_id:String,account_id:String,runtime:String,state:State<'_,AppState>)->Result<Session,String>{let mut data=state.0.lock().map_err(|e|e.to_string())?;if data.accounts.iter().all(|a|a.id!=account_id){return Err("account not found".into());}if data.sessions.iter().any(|s|s.pid==Some(pid)&&s.managed){return Err("this runtime is already attached".into());}let candidate=thread_candidates_for(&account_home(&data,&account_id)?,Some(&account_id),Some(&project_path)).into_iter().find(|c|c.thread_id==thread_id);let lineage=vec![ThreadLineageEntry{thread_id:thread_id.clone(),parent_thread_id:candidate.as_ref().and_then(|c|c.parent_thread_id.clone()),account_id:Some(account_id.clone()),rollout_path:candidate.as_ref().map(|c|c.rollout_path.clone()),discovered_at:now_iso()}];let session=Session{id:format!("session-{}",Uuid::new_v4()),name,project_path,thread_id:Some(thread_id.clone()),pid:Some(pid),runtime,status:"running".into(),account_id:Some(account_id.clone()),started_at:now_iso(),last_activity_at:now_iso(),last_message:Some("attached existing runtime".into()),managed:true,auto_failover:false,thread_lineage:lineage};data.sessions.push(session.clone());recompute_active_sessions(&mut data);append_event(&mut data,&session.id,"attach",None,Some(account_id),None,Some(thread_id),"attached existing runtime".into(),false);persist(&data)?;Ok(session)}
#[tauri::command] fn switch_session_account(session_id:String,account_id:String,state:State<'_,AppState>)->Result<Session,String>{let mut data=state.0.lock().map_err(|e|e.to_string())?;let target=data.accounts.iter().find(|a|a.id==account_id).ok_or("target account not found")?;if !target.shared_sessions_ready{return Err("target account is not prepared for shared CX sessions".into());}let (result,_)=resume_session(&mut data,&session_id,&account_id,false)?;persist(&data)?;Ok(result)}
#[tauri::command] fn supervisor_tick(state:State<'_,AppState>)->Result<SupervisorTick,String>{let mut data=state.0.lock().map_err(|e|e.to_string())?;let mut exhausted=Vec::new();let mut notices=Vec::new();let mut new_events=Vec::new();for idx in 0..data.accounts.len(){let home=expand_home(&data.accounts[idx].codex_home);let Some(log)=account_log_path(&home)else{continue};let cursor=data.accounts[idx].log_cursor;let Ok((delta,next))=read_log_delta(&log,cursor)else{continue};data.accounts[idx].log_cursor=next;if !delta.is_empty()&&quota_signal(&delta)&&data.accounts[idx].status!="exhausted"{data.accounts[idx].status="exhausted".into();data.accounts[idx].remaining_percent=Some(0);exhausted.push(data.accounts[idx].id.clone());notices.push(format!("quota signal detected for {}",data.accounts[idx].name));}}
let mut changed_sessions=Vec::new();for exhausted_id in exhausted{let ids=data.sessions.iter().filter(|s|s.managed&&s.auto_failover&&s.account_id.as_deref()==Some(exhausted_id.as_str())).map(|s|s.id.clone()).collect::<Vec<_>>();for id in ids{let Some(target)=choose_failover_account(&data,&exhausted_id)else{notices.push(format!("no ready failover account for session {id}"));continue};match resume_session(&mut data,&id,&target,true){Ok((s,e))=>{notices.push(format!("{} failed over to {}",s.name,target));changed_sessions.push(s);new_events.push(e)},Err(error)=>{let e=append_event(&mut data,&id,"failover_failed",Some(exhausted_id.clone()),Some(target.clone()),None,None,error.clone(),true);new_events.push(e);notices.push(format!("failover failed for {id}: {error}"));}}}}
let managed_ids=data.sessions.iter().filter(|s|s.managed).map(|s|s.id.clone()).collect::<Vec<_>>();for id in managed_ids{if let Some((s,e))=refresh_lineage(&mut data,&id){changed_sessions.push(s);new_events.push(e);}}
recompute_active_sessions(&mut data);let changed_accounts=data.accounts.clone();persist(&data)?;Ok(SupervisorTick{changed_sessions,changed_accounts,notices,new_events})}

#[cfg_attr(mobile,tauri::mobile_entry_point)]
pub fn run(){tauri::Builder::default().manage(AppState(Mutex::new(load_state()))).invoke_handler(tauri::generate_handler![get_overview,add_account,prepare_account,set_auto_failover,scan_existing_runtimes,discover_threads,attach_session,switch_session_account,supervisor_tick]).run(tauri::generate_context!()).expect("error while running CX Control Center");}
