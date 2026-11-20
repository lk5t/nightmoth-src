//! Cleans up after other "optimizer" tools and debloat scripts (WinUtil, O&O ShutUp10,
//! Optimizer, Tiny11-style scripts, game boosters…).
//!
//! Harmful leftovers — Windows Update or Defender switched off, core services
//! disabled, the page file removed, boot timer "tweaks", update servers blocked in the
//! hosts file — are put back automatically, each recorded in the change log with Undo.
//! Useful leftovers (telemetry blocked, Start ads off) are kept and listed.

use serde::Serialize;

use crate::changelog::{self, Change};
use crate::config::FixRecord;
use crate::state::{cfg, with_cfg};
use crate::toast;
use crate::win::{self, now, Hive, RegVal};

#[derive(Serialize, Clone, Debug)]
pub struct Finding {
    pub id: String,
    pub title: String,
    pub detail: String,
    /// "fix" = harmful, Nightmoth puts it back · "keep" = a good tweak, left in place
    pub kind: String,
    pub restart: bool,
}

fn finding(id: &str, title: &str, detail: String, kind: &str, restart: bool) -> Finding {
    Finding { id: id.into(), title: title.into(), detail, kind: kind.into(), restart }
}

fn svc(name: &str) -> String {
    format!(r"SYSTEM\CurrentControlSet\Services\{name}")
}

/// Core services and their Windows defaults (2 = automatic, 3 = manual). Fixed only when disabled (4).
const CORE_SERVICES: &[(&str, &str, u32)] = &[
    ("wuauserv", "Windows Update", 3),
    ("UsoSvc", "Update Orchestrator", 2),
    ("BITS", "Background Intelligent Transfer", 3),
    ("CryptSvc", "Cryptographic Services", 2),
    ("EventLog", "Windows Event Log", 2),
    ("Winmgmt", "Windows Management Instrumentation", 2),
    ("mpssvc", "Windows Firewall", 2),
    ("SecurityHealthService", "Windows Security", 3),
    ("Audiosrv", "Windows Audio", 2),
    ("AudioEndpointBuilder", "Audio Endpoint Builder", 2),
    ("Themes", "Themes", 2),
    ("AppXSvc", "App installs (Microsoft Store)", 3),
    ("ClipSVC", "Store licences", 3),
    ("InstallService", "Microsoft Store installs", 3),
    ("TokenBroker", "Sign-in for apps", 3),
    ("wlidsvc", "Microsoft account sign-in", 3),
    ("WSearch", "Windows Search (Start menu search)", 2),
    ("SysMain", "SysMain (app launch speed-up)", 2),
    ("ShellHWDetection", "AutoPlay / hardware detection", 2),
    ("Schedule", "Task Scheduler", 2),
    ("StateRepository", "State Repository (Start menu, apps)", 3),
    ("TimeBrokerSvc", "Time Broker (background apps)", 3),
    ("W32Time", "Windows Time", 3),
];

const WU_POLICY: &str = r"SOFTWARE\Policies\Microsoft\Windows\WindowsUpdate";
const WU_AU: &str = r"SOFTWARE\Policies\Microsoft\Windows\WindowsUpdate\AU";
const DEF_POLICY: &str = r"SOFTWARE\Policies\Microsoft\Windows Defender";
const DEF_RTP: &str = r"SOFTWARE\Policies\Microsoft\Windows Defender\Real-Time Protection";
const SYS_POLICY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System";
const SMARTSCREEN_POLICY: &str = r"SOFTWARE\Policies\Microsoft\Windows\System";
const EXPLORER: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer";
const THROTTLE: &str = r"SYSTEM\CurrentControlSet\Control\Power\PowerThrottling";
const MEMMGMT: &str = r"SYSTEM\CurrentControlSet\Control\Session Manager\Memory Management";
const DATACOLL: &str = r"SOFTWARE\Policies\Microsoft\Windows\DataCollection";
const CDM: &str = r"Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager";

/// Policy values that switch things off: (hive, key, value name, "bad" value)
const BAD_POLICIES: &[(&str, &str, &str, u32, &str)] = &[
    ("wu", WU_AU, "NoAutoUpdate", 1, "Automatic updates switched off by policy"),
    ("wu", WU_POLICY, "DisableWindowsUpdateAccess", 1, "Windows Update blocked by policy"),
    ("wu", WU_POLICY, "SetDisableUXWUAccess", 1, "Windows Update page blocked by policy"),
    ("wu", WU_POLICY, "DoNotConnectToWindowsUpdateInternetLocations", 1, "Update servers blocked by policy"),
    ("defender", DEF_POLICY, "DisableAntiSpyware", 1, "Microsoft Defender switched off by policy"),
    ("defender", DEF_POLICY, "DisableAntiVirus", 1, "Microsoft Defender antivirus switched off by policy"),
    ("defender", DEF_RTP, "DisableRealtimeMonitoring", 1, "Real-time protection switched off by policy"),
    ("defender", DEF_RTP, "DisableBehaviorMonitoring", 1, "Behaviour monitoring switched off by policy"),
    ("defender", DEF_RTP, "DisableOnAccessProtection", 1, "On-access scanning switched off by policy"),
    ("defender", DEF_RTP, "DisableIOAVProtection", 1, "Download scanning switched off by policy"),
    ("smartscreen", SMARTSCREEN_POLICY, "EnableSmartScreen", 0, "SmartScreen switched off by policy"),
    ("throttle", THROTTLE, "PowerThrottlingOff", 1, "Power throttling switched off (drains laptops, breaks Efficiency mode)"),
    ("mitigations", MEMMGMT, "FeatureSettingsOverride", 3, "Spectre/Meltdown protection switched off"),
];

/// Hosts-file entries that break updates, the Store or activation (telemetry blocks are fine).
const BAD_HOSTS: &[&str] = &[
    "windowsupdate.com",
    "update.microsoft.com",
    "windowsupdate.microsoft.com",
    "delivery.mp.microsoft.com",
    "dl.delivery.mp.microsoft.com",
    "displaycatalog.mp.microsoft.com",
    "licensing.mp.microsoft.com",
    "storeedgefd.dsx.mp.microsoft.com",
    "login.live.com",
    "activation.sls.microsoft.com",
    "go.microsoft.com",
    "definitionupdates.microsoft.com",
    "wdcp.microsoft.com",
];

fn hosts_path() -> std::path::PathBuf {
    std::path::PathBuf::from(win::env("WINDIR")).join(r"System32\drivers\etc\hosts")
}

fn bad_host_line(line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() || t.starts_with('#') {
        return false;
    }
    let low = t.to_lowercase();
    let mut parts = low.split_whitespace();
    let _ip = parts.next();
    parts.any(|h| BAD_HOSTS.iter().any(|b| h == *b || h.ends_with(&format!(".{b}"))))
}

/// bcdedit "tweaks" that cause stutter or timer problems on modern PCs.
const BAD_BCD: &[&str] = &["useplatformclock", "useplatformtick", "disabledynamictick", "tscsyncpolicy"];

fn bcd_values() -> Vec<(String, String)> {
    let out = win::run("bcdedit.exe", &["/enum", "{current}"]).unwrap_or_default();
    out.lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let k = it.next()?.to_lowercase();
            let v = it.collect::<Vec<_>>().join(" ");
            BAD_BCD.contains(&k.as_str()).then_some((k, v))
        })
        .collect()
}

fn autotuning_disabled() -> bool {
    win::run("netsh.exe", &["int", "tcp", "show", "global"])
        .map(|o| o.lines().any(|l| l.to_lowercase().contains("auto-tuning") && l.to_lowercase().trim_end().ends_with("disabled")))
        .unwrap_or(false)
}

fn pagefile_off() -> bool {
    let v = win::ps_json(
        "$cs = Get-CimInstance Win32_ComputerSystem; $pf = @(Get-CimInstance Win32_PageFileSetting).Count; \
         ConvertTo-Json @{ auto = [bool]$cs.AutomaticManagedPagefile; files = $pf } -Compress",
    );
    v.first().map(|x| !x["auto"].as_bool().unwrap_or(true) && x["files"].as_u64().unwrap_or(1) == 0).unwrap_or(false)
}

/// Exclusions so broad they switch Defender off for the whole drive or Windows itself.
fn bad_exclusions() -> Vec<String> {
    let v = win::ps_json("ConvertTo-Json @((Get-MpPreference).ExclusionPath) -Compress");
    let windir = win::env("WINDIR").to_lowercase();
    let profile = win::env("USERPROFILE").to_lowercase();
    v.iter()
        .filter_map(|x| x.as_str().map(|s| s.to_string()))
        .filter(|p| {
            let l = p.trim_end_matches('\\').to_lowercase();
            l.len() <= 2 || l == "*" || l == windir || l == profile || l == format!("{windir}\\system32") || l.ends_with(":\\*")
        })
        .collect()
}

/// Look for everything, change nothing.
pub fn scan() -> Vec<Finding> {
    let mut out = vec![];
    for (name, label, _) in CORE_SERVICES {
        if win::reg_dword(Hive::HKLM, &svc(name), "Start") == Some(4) {
            out.push(finding(&format!("svc:{name}"), &format!("{label} service disabled"), format!("Service {name} was switched off"), "fix", false));
        }
    }
    for (group, key, name, bad, label) in BAD_POLICIES {
        if win::reg_dword(Hive::HKLM, key, name) == Some(*bad) {
            out.push(finding(&format!("pol:{name}"), label, format!(r"HKLM\{key}\{name}"), "fix", *group == "mitigations"));
        }
    }
    if win::reg_dword(Hive::HKLM, SYS_POLICY, "EnableLUA") == Some(0) {
        out.push(finding("uac", "User Account Control switched off", "Every app runs with full admin rights".into(), "fix", true));
    } else if win::reg_dword(Hive::HKLM, SYS_POLICY, "ConsentPromptBehaviorAdmin") == Some(0) {
        out.push(finding("uac-prompt", "Admin prompts switched off", "Apps get admin rights without asking".into(), "fix", false));
    }
    if win::reg_str(Hive::HKLM, EXPLORER, "SmartScreenEnabled").map(|v| v.eq_ignore_ascii_case("off")).unwrap_or(false) {
        out.push(finding("smartscreen-exp", "SmartScreen switched off", "Downloaded apps aren't checked".into(), "fix", false));
    }
    let hosts = std::fs::read_to_string(hosts_path()).unwrap_or_default();
    let bad_hosts: Vec<&str> = hosts.lines().filter(|l| bad_host_line(l)).collect();
    if !bad_hosts.is_empty() {
        out.push(finding("hosts", "Microsoft update servers blocked in the hosts file", format!("{} entries", bad_hosts.len()), "fix", false));
    }
    for (k, v) in bcd_values() {
        out.push(finding(&format!("bcd:{k}"), "Boot timer \"tweak\" (can cause stutter)", format!("{k} = {v}"), "fix", true));
    }
    if autotuning_disabled() {
        out.push(finding("autotuning", "Network auto-tuning switched off", "Slows downloads on fast connections".into(), "fix", false));
    }
    if pagefile_off() {
        out.push(finding("pagefile", "Page file removed", "Apps and games can crash when memory runs out".into(), "fix", true));
    }
    let ex = bad_exclusions();
    if !ex.is_empty() {
        out.push(finding("exclusions", "Defender told to ignore whole drives", ex.join(", "), "fix", false));
    }
    // good leftovers, kept as they are
    if win::reg_dword(Hive::HKLM, DATACOLL, "AllowTelemetry") == Some(0) {
        out.push(finding("keep:telemetry", "Telemetry blocked", "Kept — that's a good one".into(), "keep", false));
    }
    if win::reg_dword(Hive::HKCU, CDM, "SystemPaneSuggestionsEnabled") == Some(0) {
        out.push(finding("keep:ads", "Start menu suggestions off", "Kept — that's a good one".into(), "keep", false));
    }
    out
}

fn reg_fix(hive: Hive, key: &str, name: &str, to: Option<RegVal>) -> Result<Vec<Change>, String> {
    Ok(match to {
        Some(v) => changelog::reg(hive, key, name, v)?.into_iter().collect(),
        None => changelog::reg_del(hive, key, name)?.into_iter().collect(),
    })
}

/// Put one thing back. Returns the changes (for undo).
fn fix(f: &Finding) -> Result<Vec<Change>, String> {
    let id = f.id.as_str();
    if let Some(name) = id.strip_prefix("svc:") {
        let (_, _, start) = CORE_SERVICES.iter().find(|s| s.0 == name).ok_or("unknown service")?;
        let ch = reg_fix(Hive::HKLM, &svc(name), "Start", Some(RegVal::Dword(*start)))?;
        if *start == 2 {
            let _ = win::run("sc.exe", &["start", name]);
        }
        return Ok(ch);
    }
    if let Some(name) = id.strip_prefix("pol:") {
        let (_, key, _, _, _) = BAD_POLICIES.iter().find(|p| p.2 == name).ok_or("unknown policy")?;
        let mut ch = reg_fix(Hive::HKLM, key, name, None)?;
        if name == "FeatureSettingsOverride" {
            ch.extend(reg_fix(Hive::HKLM, MEMMGMT, "FeatureSettingsOverrideMask", None)?);
        }
        return Ok(ch);
    }
    if let Some(k) = id.strip_prefix("bcd:") {
        let before = bcd_values().into_iter().find(|(n, _)| n == k).map(|(_, v)| v).unwrap_or_default();
        win::run("bcdedit.exe", &["/deletevalue", "{current}", k])?;
        let undo_val = match before.to_lowercase().as_str() {
            "yes" => "true".to_string(),
            "no" => "false".to_string(),
            v => v.to_string(),
        };
        return Ok(vec![Change::Command { program: "bcdedit.exe".into(), args: vec!["/set".into(), "{current}".into(), k.into(), undo_val] }]);
    }
    match id {
        "uac" => reg_fix(Hive::HKLM, SYS_POLICY, "EnableLUA", Some(RegVal::Dword(1))),
        "uac-prompt" => reg_fix(Hive::HKLM, SYS_POLICY, "ConsentPromptBehaviorAdmin", Some(RegVal::Dword(5))),
        "smartscreen-exp" => reg_fix(Hive::HKLM, EXPLORER, "SmartScreenEnabled", Some(RegVal::Str("Warn".into()))),
        "hosts" => {
            let p = hosts_path();
            let text = std::fs::read_to_string(&p).map_err(|e| e.to_string())?;
            let backup = win::data_dir().join(format!("hosts.backup-{}", now()));
            std::fs::write(&backup, &text).map_err(|e| e.to_string())?;
            let fixed: Vec<String> = text
                .lines()
                .map(|l| if bad_host_line(l) { format!("# disabled by Nightmoth (blocked updates): {l}") } else { l.to_string() })
                .collect();
            std::fs::write(&p, fixed.join("\r\n") + "\r\n").map_err(|e| e.to_string())?;
            let _ = win::run("ipconfig.exe", &["/flushdns"]);
            Ok(vec![Change::FileRestore { path: p.to_string_lossy().into(), backup: backup.to_string_lossy().into() }])
        }
        "autotuning" => {
            win::run("netsh.exe", &["int", "tcp", "set", "global", "autotuninglevel=normal"])?;
            Ok(vec![Change::Command {
                program: "netsh.exe".into(),
                args: ["int", "tcp", "set", "global", "autotuninglevel=disabled"].iter().map(|s| s.to_string()).collect(),
            }])
        }
        "pagefile" => {
            win::ps("$cs = Get-CimInstance Win32_ComputerSystem; Set-CimInstance -InputObject $cs -Property @{AutomaticManagedPagefile=$true}")?;
            Ok(vec![Change::Command {
                program: "powershell.exe".into(),
                args: ["-NoProfile", "-Command", "$cs = Get-CimInstance Win32_ComputerSystem; Set-CimInstance -InputObject $cs -Property @{AutomaticManagedPagefile=$false}"]
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            }])
        }
        "exclusions" => {
            let mut ch = vec![];
            for p in bad_exclusions() {
                let q = win::ps_quote(&p);
                if win::ps(&format!("Remove-MpPreference -ExclusionPath {q}")).is_ok() {
                    ch.push(Change::Command {
                        program: "powershell.exe".into(),
                        args: vec!["-NoProfile".into(), "-Command".into(), format!("Add-MpPreference -ExclusionPath {q}")],
                    });
                }
            }
            Ok(ch)
        }
        _ => Err("nothing to do".into()),
    }
}

fn remember(f: &Finding, action: &str) {
    with_cfg(|c| {
        c.fixes.retain(|x| x.id != f.id || x.action != action);
        c.fixes.push(FixRecord { id: f.id.clone(), title: f.title.clone(), detail: f.detail.clone(), t: now(), action: action.into() });
        if c.fixes.len() > 150 {
            c.fixes.remove(0);
        }
    });
}

/// Fix everything harmful. Returns (fixed, needs restart, errors).
pub fn fix_all(module: &str) -> (Vec<Finding>, bool, Vec<String>) {
    let found = scan();
    let bad: Vec<&Finding> = found.iter().filter(|f| f.kind == "fix").collect();
    for k in found.iter().filter(|f| f.kind == "keep") {
        if !cfg().fixes.iter().any(|x| x.id == k.id) {
            remember(k, "kept");
        }
    }
    if bad.is_empty() {
        return (vec![], false, vec![]);
    }
    let _ = crate::speed::restore_point(false);
    let mut fixed = vec![];
    let mut errs = vec![];
    let mut restart = false;
    for f in bad {
        match fix(f) {
            Ok(ch) => {
                changelog::record_many(module, &format!("Fixed: {}", f.title), "Left behind by another optimizer tool", ch);
                remember(f, "fixed");
                restart |= f.restart;
                fixed.push(f.clone());
            }
            Err(e) => {
                remember(f, &format!("couldn't fix: {e}"));
                errs.push(format!("{}: {e}", f.title));
            }
        }
    }
    (fixed, restart, errs)
}

/// Autopilot: fix silently, tell the user with a pop-up.
pub fn run_auto() {
    let (fixed, restart, _) = fix_all("Autopilot");
    if fixed.is_empty() {
        return;
    }
    let first = &fixed[0].title;
    let body = if fixed.len() == 1 { first.clone() } else { format!("{first} and {} more", fixed.len() - 1) };
    let body = if restart { format!("{body} · restart to finish") } else { body };
    toast::show("fixer", &format!("Repaired {} thing{} another tool broke", fixed.len(), if fixed.len() == 1 { "" } else { "s" }), &body);
}
