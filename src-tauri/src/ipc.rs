//! App <-> engine link.
//!
//! WebView2 (the window) can't run with admin rights, so Nightmoth is two processes:
//! - the app: windows, tray, pop-ups, runs as the normal user;
//! - the engine: `nightmoth.exe --engine`, elevated, does every system change and
//!   runs Autopilot. It's started by the "Nightmoth Engine" scheduled task (set up
//!   once with a UAC prompt), so later launches need no prompt.
//!
//! They talk over 127.0.0.1 with a random port and a random secret token, kept in
//! `%ProgramData%\Nightmoth\engine.json` (only admins can write there). Because any
//! program running as the same user could read that token, the engine also checks
//! which process is on the other end of every connection and only answers
//! Nightmoth's own exe. It only accepts its fixed list of commands.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::log::log;
use crate::win::{self, wide};

use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::System::Threading::WaitForSingleObject;
use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};

pub const TASK: &str = "Nightmoth Engine";

#[derive(Serialize, Deserialize)]
struct Endpoint {
    port: u16,
    token: String,
    pid: u32,
}

#[derive(Serialize, Deserialize)]
struct Request {
    token: String,
    cmd: String,
    args: Value,
}

#[derive(Serialize, Deserialize)]
struct Response {
    ok: bool,
    value: Value,
    error: String,
}

fn endpoint_path() -> std::path::PathBuf {
    win::engine_dir().join("engine.json")
}

/// Which process owns the client end of a loopback connection to `server_port`?
fn peer_pid(peer: SocketAddr, server_port: u16) -> Option<u32> {
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GetExtendedTcpTable, MIB_TCPROW_OWNER_PID, MIB_TCPTABLE_OWNER_PID, TCP_TABLE_OWNER_PID_CONNECTIONS,
    };
    const AF_INET: u32 = 2;
    let mut size: u32 = 0;
    unsafe {
        GetExtendedTcpTable(std::ptr::null_mut(), &mut size, 0, AF_INET, TCP_TABLE_OWNER_PID_CONNECTIONS, 0);
    }
    for _ in 0..4 {
        // u32 buffer keeps the table correctly aligned
        let mut buf = vec![0u32; (size as usize / 4) + 256];
        let mut len = (buf.len() * 4) as u32;
        let rc = unsafe {
            GetExtendedTcpTable(buf.as_mut_ptr() as *mut _, &mut len, 0, AF_INET, TCP_TABLE_OWNER_PID_CONNECTIONS, 0)
        };
        if rc == 122 {
            // ERROR_INSUFFICIENT_BUFFER: the table grew, try again
            size = len;
            continue;
        }
        if rc != 0 {
            return None;
        }
        let rows: &[MIB_TCPROW_OWNER_PID] = unsafe {
            let table = buf.as_ptr() as *const MIB_TCPTABLE_OWNER_PID;
            std::slice::from_raw_parts((*table).table.as_ptr(), (*table).dwNumEntries as usize)
        };
        return rows
            .iter()
            .find(|r| u16::from_be(r.dwLocalPort as u16) == peer.port() && u16::from_be(r.dwRemotePort as u16) == server_port)
            .map(|r| r.dwOwningPid);
    }
    None
}

/// Full path of a process's exe.
fn pid_exe(pid: u32) -> Option<String> {
    use windows_sys::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION};
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut len);
        CloseHandle(h);
        if ok == 0 {
            return None;
        }
        Some(String::from_utf16_lossy(&buf[..len as usize]))
    }
}

/// Is the client Nightmoth itself (the same exe file as this engine)?
/// Paths are compared in their full, real form, so short names (PROGRA~1), letter case
/// or a different way of writing the same path don't lock the real app out.
fn trusted_client(peer: SocketAddr, server_port: u16) -> bool {
    let Ok(me) = std::env::current_exe() else { return false };
    let real = |p: &std::path::Path| std::fs::canonicalize(p).map(|c| c.to_string_lossy().to_lowercase()).unwrap_or_else(|_| p.to_string_lossy().to_lowercase());
    let mine = real(&me);
    let Some(pid) = peer_pid(peer, server_port) else {
        log(&format!("engine: couldn't tell which program connected (port {})", peer.port()));
        return false;
    };
    match pid_exe(pid) {
        Some(exe) => {
            let theirs = real(std::path::Path::new(&exe));
            if theirs == mine {
                true
            } else {
                log(&format!("engine: refused a connection from another program: {exe} (engine is {})", me.to_string_lossy()));
                false
            }
        }
        None => {
            log(&format!("engine: couldn't read the path of process {pid}"));
            false
        }
    }
}

fn same_token(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

// ================================================================== engine side

/// Run the engine's server loop (blocks forever).
pub fn serve(dispatch: fn(&str, &Value) -> Result<Value, String>) {
    let listener = match TcpListener::bind("127.0.0.1:0") {
        Ok(l) => l,
        Err(e) => {
            log(&format!("engine: can't listen: {e}"));
            std::process::exit(1);
        }
    };
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
    let token: String = (0..48).map(|_| format!("{:x}", rand::random::<u8>() % 16)).collect();
    let ep = Endpoint { port, token: token.clone(), pid: std::process::id() };
    let _ = win::data_dir(); // creates and locks the engine folder
    if let Err(e) = std::fs::write(endpoint_path(), serde_json::to_string(&ep).unwrap_or_default()) {
        log(&format!("engine: can't write endpoint: {e}"));
    }
    log(&format!("engine ready on port {port}"));

    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let token = token.clone();
        std::thread::spawn(move || handle(stream, &token, port, dispatch));
    }
}

fn handle(stream: TcpStream, token: &str, port: u16, dispatch: fn(&str, &Value) -> Result<Value, String>) {
    // only local connections, and only from Nightmoth's own exe
    let Ok(peer) = stream.peer_addr() else { return };
    if !peer.ip().is_loopback() {
        return;
    }
    if !trusted_client(peer, port) {
        return;
    }
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    });
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }
    let resp = match serde_json::from_str::<Request>(&line) {
        Ok(req) if same_token(&req.token, token) => {
            let res = std::panic::catch_unwind(|| dispatch(&req.cmd, &req.args))
                .unwrap_or_else(|_| Err(format!("Engine error while running {}", req.cmd)));
            match res {
                Ok(v) => Response { ok: true, value: v, error: String::new() },
                Err(e) => Response { ok: false, value: Value::Null, error: e },
            }
        }
        Ok(_) => Response { ok: false, value: Value::Null, error: "bad token".into() },
        Err(e) => Response { ok: false, value: Value::Null, error: format!("bad request: {e}") },
    };
    let mut w = stream;
    let _ = w.write_all(format!("{}\n", serde_json::to_string(&resp).unwrap_or_default()).as_bytes());
}

/// Is another engine already answering?
pub fn engine_alive() -> bool {
    matches!(try_call("ping", &json!({}), Duration::from_secs(3)), Ok(Ok(_)))
}

// ================================================================== app side

/// Ok(result) if we reached the engine, Err(io) if we couldn't connect.
fn try_call(cmd: &str, args: &Value, timeout: Duration) -> Result<Result<Value, String>, String> {
    let ep: Endpoint = std::fs::read_to_string(endpoint_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .ok_or("engine not started")?;
    let addr = SocketAddr::from(([127, 0, 0, 1], ep.port));
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_millis(1500)).map_err(|e| e.to_string())?;
    let _ = s.set_read_timeout(Some(timeout));
    let req = Request { token: ep.token, cmd: cmd.into(), args: args.clone() };
    s.write_all(format!("{}\n", serde_json::to_string(&req).map_err(|e| e.to_string())?).as_bytes())
        .map_err(|e| e.to_string())?;
    let mut line = String::new();
    BufReader::new(s).read_line(&mut line).map_err(|e| e.to_string())?;
    let r: Response = serde_json::from_str(&line).map_err(|e| format!("bad engine reply: {e}"))?;
    Ok(if r.ok { Ok(r.value) } else { Err(r.error) })
}

/// Call the engine, starting it if needed. Long operations (uninstalls, restore points) are allowed.
pub fn call(cmd: &str, args: Value) -> Result<Value, String> {
    match try_call(cmd, &args, Duration::from_secs(600)) {
        Ok(r) => r,
        Err(_) => {
            ensure_engine()?;
            try_call(cmd, &args, Duration::from_secs(600)).map_err(|e| format!("Can't reach the Nightmoth engine: {e}"))?
        }
    }
}

/// Call without trying to start the engine (for background polling).
pub fn call_quiet(cmd: &str, args: Value) -> Result<Value, String> {
    try_call(cmd, &args, Duration::from_secs(20))?
}

static STARTING: Mutex<()> = Mutex::new(());
/// The admin prompt is shown at most once per app run.
static PROMPTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static LAST_ERROR: Mutex<String> = Mutex::new(String::new());

/// "Try again" from the diagnostics screen: allow one more admin prompt.
pub fn allow_retry() {
    PROMPTED.store(false, std::sync::atomic::Ordering::Relaxed);
}

pub fn last_error() -> String {
    LAST_ERROR.lock().map(|e| e.clone()).unwrap_or_default()
}

/// The Windows account ID (SID) of whoever runs the app, so the engine task is set up for
/// exactly this account — names can be ambiguous (Microsoft / work accounts).
fn current_sid() -> String {
    win::run("whoami.exe", &["/user", "/fo", "csv", "/nh"])
        .ok()
        .and_then(|o| o.split('"').filter(|x| x.starts_with("S-1-")).next().map(String::from))
        .unwrap_or_default()
}

fn tail(path: &std::path::Path, n: usize) -> String {
    let s = std::fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = s.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

/// Why the engine can't be reached, in words.
fn diagnose() -> String {
    let elog = tail(&win::engine_dir().join("nightmoth.log"), 60);
    let last_lines: Vec<&str> = elog.lines().rev().take(25).collect();
    if let Some(l) = last_lines.iter().find(|l| l.contains("refused a connection")) {
        return format!("The engine is running but didn't recognise the app ({}). Reinstall Nightmoth into Program Files and start it from the Start menu.", l.split("] ").nth(1).unwrap_or(l));
    }
    if let Some(l) = last_lines.iter().find(|l| l.contains("task setup failed") || l.contains("PANIC")) {
        return format!("The engine couldn't start: {}", l.split("] ").nth(1).unwrap_or(l));
    }
    if !elog.contains("engine ready") {
        return "The engine never started. Your antivirus may have blocked it — allow Nightmoth, then press Try again.".into();
    }
    "The engine started but the app can't reach it. Press Try again; if it keeps happening, copy the details and send them.".into()
}

/// Start the engine and wait until it answers. At most one admin prompt per app run.
pub fn ensure_engine() -> Result<(), String> {
    let _guard = STARTING.lock().unwrap_or_else(|e| e.into_inner());
    if engine_alive() {
        return Ok(());
    }
    log("app: starting engine via scheduled task");
    if win::run("schtasks.exe", &["/Run", "/TN", TASK]).is_ok() && wait_alive(Duration::from_secs(20)) {
        return Ok(());
    }
    if PROMPTED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        let e = last_error();
        return Err(if e.is_empty() { "The Nightmoth engine isn't running".into() } else { e });
    }
    // one admin prompt: registers the engine task for this account AND starts the engine
    let sid = current_sid();
    log(&format!("app: setting up the engine (one admin prompt) for {sid}"));
    if let Err(e) = run_elevated(&format!("--setup-engine {sid}")) {
        log(&format!("app: setup declined/failed: {e}"));
        let msg = "Nightmoth needs admin permission once to set up its engine. Press Try again and click Yes on the Windows prompt.".to_string();
        *LAST_ERROR.lock().unwrap_or_else(|e| e.into_inner()) = msg.clone();
        return Err(msg);
    }
    if wait_alive(Duration::from_secs(40)) {
        return Ok(());
    }
    let msg = diagnose();
    log(&format!("app: engine unreachable: {msg}"));
    *LAST_ERROR.lock().unwrap_or_else(|e| e.into_inner()) = msg.clone();
    Err(msg)
}

/// Everything useful for figuring out a broken engine, for the diagnostics screen.
pub fn diagnostics() -> serde_json::Value {
    let task = win::run("schtasks.exe", &["/Query", "/TN", TASK, "/V", "/FO", "LIST"]).unwrap_or_else(|e| format!("not found: {e}"));
    json!({
        "error": last_error(),
        "alive": engine_alive(),
        "endpoint": endpoint_path().exists(),
        "app_exe": std::env::current_exe().map(|e| e.to_string_lossy().to_string()).unwrap_or_default(),
        "task": task.lines().filter(|l| ["TaskName", "Status", "Last Run Time", "Last Result", "Task To Run", "Run As User"].iter().any(|k| l.trim_start().starts_with(k))).collect::<Vec<_>>().join("\n"),
        "engine_log": tail(&win::engine_dir().join("nightmoth.log"), 40),
        "app_log": tail(&win::user_dir().join("nightmoth.log"), 40),
    })
}

fn wait_alive(max: Duration) -> bool {
    let t = Instant::now();
    while t.elapsed() < max {
        if engine_alive() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(400));
    }
    false
}

fn shell_runas(arg: &str, wait: bool) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let file = wide(&exe.to_string_lossy());
    let verb = wide("runas");
    let params = wide(arg);
    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOCLOSEPROCESS;
    info.lpVerb = verb.as_ptr();
    info.lpFile = file.as_ptr();
    info.lpParameters = params.as_ptr();
    info.nShow = 0; // SW_HIDE
    let ok = unsafe { ShellExecuteExW(&mut info) };
    if ok == 0 {
        return Err("permission was not given".into());
    }
    if !info.hProcess.is_null() {
        if wait {
            unsafe {
                WaitForSingleObject(info.hProcess, 120_000);
            }
        }
        unsafe {
            CloseHandle(info.hProcess);
        }
    }
    Ok(())
}

fn run_elevated(arg: &str) -> Result<(), String> {
    shell_runas(arg, true)
}


/// `nightmoth.exe --setup-engine <SID>` (elevated, one prompt): create the scheduled task
/// that runs the engine elevated at logon for this account, then start the engine now.
pub fn setup_engine(sid: Option<&str>) -> Result<(), String> {
    let exe_path = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe = win::ps_quote(&exe_path.to_string_lossy());
    let user = match sid {
        Some(s) if s.starts_with("S-1-") && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') => format!("'{s}'"),
        _ => "\"$env:USERDOMAIN\\$env:USERNAME\"".to_string(),
    };
    let script = format!(
        "Unregister-ScheduledTask -TaskName 'Nightmoth' -Confirm:$false -ErrorAction SilentlyContinue; \
         $a = New-ScheduledTaskAction -Execute {exe} -Argument '--engine'; \
         $p = New-ScheduledTaskPrincipal -UserId {user} -LogonType Interactive -RunLevel Highest; \
         $s = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit ([TimeSpan]::Zero) -MultipleInstances IgnoreNew -Priority 5; \
         Register-ScheduledTask -TaskName '{TASK}' -Action $a -Principal $p -Settings $s -Force | Out-Null"
    );
    let registered = win::ps(&script);
    if let Err(e) = &registered {
        log(&format!("engine task setup failed: {e}"));
    }
    // start the engine right away from this elevated process (works even if the task couldn't be made)
    if !engine_alive() {
        use std::os::windows::process::CommandExt;
        let spawned = std::process::Command::new(&exe_path)
            .arg("--engine")
            .creation_flags(win::CREATE_NO_WINDOW | 0x0000_0008 /* DETACHED_PROCESS */)
            .spawn();
        match spawned {
            Ok(_) => log("engine started by setup"),
            Err(e) => log(&format!("engine start by setup failed: {e}")),
        }
    }
    registered.map(|_| ())
}
