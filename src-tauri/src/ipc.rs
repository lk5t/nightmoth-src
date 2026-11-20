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
fn trusted_client(peer: SocketAddr, server_port: u16) -> bool {
    let Ok(me) = std::env::current_exe() else { return false };
    let me = me.to_string_lossy().to_lowercase();
    match peer_pid(peer, server_port).and_then(pid_exe) {
        Some(exe) => exe.to_lowercase() == me,
        None => false,
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
        log(&format!("engine: refused a connection from another program (port {})", peer.port()));
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

/// Start the engine and wait until it answers.
pub fn ensure_engine() -> Result<(), String> {
    let _guard = STARTING.lock().unwrap_or_else(|e| e.into_inner());
    if engine_alive() {
        return Ok(());
    }
    log("app: starting engine via scheduled task");
    if win::run("schtasks.exe", &["/Run", "/TN", TASK]).is_ok() && wait_alive(Duration::from_secs(12)) {
        return Ok(());
    }
    // task missing or broken: set it up (one UAC prompt), then start it
    log("app: setting up engine task (UAC)");
    match run_elevated("--setup-engine") {
        Ok(_) => {
            if win::run("schtasks.exe", &["/Run", "/TN", TASK]).is_ok() && wait_alive(Duration::from_secs(15)) {
                return Ok(());
            }
        }
        Err(e) => {
            log(&format!("app: setup declined/failed: {e}"));
            return Err("Nightmoth needs admin permission once to set up its engine. Click Yes on the Windows prompt (open Nightmoth again to retry).".into());
        }
    }
    // last resort: start the engine directly (UAC prompt)
    log("app: starting engine directly (UAC)");
    let _ = run_elevated_nowait("--engine");
    if wait_alive(Duration::from_secs(20)) {
        Ok(())
    } else {
        Err("The Nightmoth engine didn't start. Details are in %APPDATA%\\Nightmoth\\nightmoth.log".into())
    }
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

fn run_elevated_nowait(arg: &str) -> Result<(), String> {
    shell_runas(arg, false)
}

/// `nightmoth.exe --setup-engine` (elevated): create the scheduled task that runs the engine elevated.
pub fn setup_engine() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe = win::ps_quote(&exe.to_string_lossy());
    let script = format!(
        "Unregister-ScheduledTask -TaskName 'Nightmoth' -Confirm:$false -ErrorAction SilentlyContinue; \
         $a = New-ScheduledTaskAction -Execute {exe} -Argument '--engine'; \
         $p = New-ScheduledTaskPrincipal -UserId \"$env:USERDOMAIN\\$env:USERNAME\" -LogonType Interactive -RunLevel Highest; \
         $s = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -ExecutionTimeLimit ([TimeSpan]::Zero) -MultipleInstances IgnoreNew -Priority 5; \
         Register-ScheduledTask -TaskName '{TASK}' -Action $a -Principal $p -Settings $s -Force | Out-Null"
    );
    win::ps(&script).map(|_| ())
}
