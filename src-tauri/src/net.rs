//! Network: find the fastest DNS server from this PC and switch to it (with Undo),
//! and measure ping.

use serde::Serialize;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

use crate::changelog::{self, Change};
use crate::toast;
use crate::win;

pub const SERVERS: &[(&str, &str, &str)] = &[
    ("Cloudflare", "1.1.1.1", "1.0.0.1"),
    ("Google", "8.8.8.8", "8.8.4.4"),
    ("Quad9", "9.9.9.9", "149.112.112.112"),
    ("OpenDNS", "208.67.222.222", "208.67.220.220"),
    ("AdGuard (blocks ads)", "94.140.14.14", "94.140.15.15"),
];

const DOMAINS: &[&str] = &[
    "google.com", "youtube.com", "roblox.com", "discord.com", "steampowered.com", "microsoft.com", "reddit.com",
    "twitch.tv", "epicgames.com", "spotify.com", "amazon.com", "wikipedia.org",
];

#[derive(Serialize, Clone, Debug)]
pub struct DnsResult {
    pub name: String,
    pub primary: String,
    pub secondary: String,
    pub ms: f64,
    pub failed: u32,
    pub current: bool,
}

fn query(server: Ipv4Addr, domain: &str, id: u16) -> Option<f64> {
    let mut pkt = vec![(id >> 8) as u8, id as u8, 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0];
    for label in domain.split('.') {
        pkt.push(label.len() as u8);
        pkt.extend_from_slice(label.as_bytes());
    }
    pkt.extend_from_slice(&[0, 0, 1, 0, 1]);
    let sock = UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.set_read_timeout(Some(Duration::from_millis(1200))).ok()?;
    let t = Instant::now();
    sock.send_to(&pkt, SocketAddr::from((server, 53))).ok()?;
    let mut buf = [0u8; 512];
    loop {
        let (n, _) = sock.recv_from(&mut buf).ok()?;
        if n >= 2 && buf[0] == (id >> 8) as u8 && buf[1] == id as u8 {
            return Some(t.elapsed().as_secs_f64() * 1000.0);
        }
    }
}

fn bench(ip: &str) -> (f64, u32) {
    let Ok(addr) = ip.parse::<Ipv4Addr>() else { return (9999.0, DOMAINS.len() as u32) };
    let mut times = vec![];
    let mut failed = 0;
    for (i, d) in DOMAINS.iter().enumerate() {
        match query(addr, d, 0x4e00 + i as u16) {
            Some(ms) => times.push(ms),
            None => {
                failed += 1;
                times.push(1200.0);
            }
        }
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    (times[times.len() / 2], failed)
}

/// DNS servers set on the adapters that reach the internet: (interface index, servers, set by hand?)
fn adapters() -> Vec<(u64, Vec<String>, bool)> {
    let script = "$r = foreach ($c in Get-NetIPConfiguration | Where-Object { $_.IPv4DefaultGateway }) { \
        $a = Get-NetAdapter -InterfaceIndex $c.InterfaceIndex; \
        $ns = (Get-ItemProperty \"HKLM:\\SYSTEM\\CurrentControlSet\\Services\\Tcpip\\Parameters\\Interfaces\\$($a.InterfaceGuid)\" -ErrorAction SilentlyContinue).NameServer; \
        [pscustomobject]@{ idx = $c.InterfaceIndex; dns = @($c.DNSServer | Where-Object { $_.AddressFamily -eq 2 } | ForEach-Object { $_.ServerAddresses }); manual = [bool]$ns } }; \
        ConvertTo-Json -InputObject @($r) -Compress -Depth 4";
    win::ps_json(script)
        .into_iter()
        .filter_map(|v| {
            let idx = v["idx"].as_u64()?;
            let dns: Vec<String> = v["dns"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
            Some((idx, dns, v["manual"].as_bool().unwrap_or(false)))
        })
        .collect()
}

/// Time every known server plus the one you use now.
pub fn benchmark() -> Vec<DnsResult> {
    let current = adapters().into_iter().next().map(|a| a.1).unwrap_or_default();
    let cur_primary = current.first().cloned().unwrap_or_default();
    let mut out: Vec<DnsResult> = SERVERS
        .iter()
        .map(|(n, p, s)| {
            let (ms, failed) = bench(p);
            DnsResult { name: n.to_string(), primary: p.to_string(), secondary: s.to_string(), ms, failed, current: *p == cur_primary }
        })
        .collect();
    if !cur_primary.is_empty() && !out.iter().any(|r| r.current) {
        let (ms, failed) = bench(&cur_primary);
        out.push(DnsResult {
            name: "Your current DNS".into(),
            primary: cur_primary.clone(),
            secondary: current.get(1).cloned().unwrap_or_default(),
            ms,
            failed,
            current: true,
        });
    }
    out.sort_by(|a, b| (a.failed, a.ms as u64).cmp(&(b.failed, b.ms as u64)));
    out
}

/// Switch every internet adapter to these servers. Undo puts back exactly what was there.
pub fn set_dns(name: &str, primary: &str, secondary: &str, module: &str) -> Result<(), String> {
    for ip in [primary, secondary] {
        if !ip.is_empty() && ip.parse::<Ipv4Addr>().is_err() {
            return Err("Not a valid DNS address".into());
        }
    }
    let mut ch = vec![];
    for (idx, before, manual) in adapters() {
        let list = if secondary.is_empty() { format!("'{primary}'") } else { format!("'{primary}','{secondary}'") };
        win::ps(&format!("Set-DnsClientServerAddress -InterfaceIndex {idx} -ServerAddresses ({list})"))?;
        let undo = if manual && !before.is_empty() {
            format!("Set-DnsClientServerAddress -InterfaceIndex {idx} -ServerAddresses ({})", before.iter().map(|b| format!("'{b}'")).collect::<Vec<_>>().join(","))
        } else {
            format!("Set-DnsClientServerAddress -InterfaceIndex {idx} -ResetServerAddresses")
        };
        ch.push(Change::Command { program: "powershell.exe".into(), args: vec!["-NoProfile".into(), "-Command".into(), undo] });
    }
    let _ = win::run("ipconfig.exe", &["/flushdns"]);
    changelog::record_many(module, &format!("DNS switched to {name}"), &format!("{primary} {secondary}"), ch);
    Ok(())
}

/// Autopilot: switch only if another server is clearly (20 %+) faster and reliable.
pub fn dns_auto() -> Result<(), String> {
    let res = benchmark();
    let cur = res.iter().find(|r| r.current).cloned();
    let best = res.iter().find(|r| r.failed == 0).cloned().ok_or("no DNS answered")?;
    if best.current {
        return Ok(());
    }
    if let Some(c) = &cur {
        if c.failed == 0 && best.ms > c.ms * 0.8 {
            return Ok(());
        }
    }
    set_dns(&best.name, &best.primary, &best.secondary, "Autopilot")?;
    toast::show("network", &format!("DNS switched to {}", best.name), &format!("{:.0} ms vs {:.0} ms before", best.ms, cur.map(|c| c.ms).unwrap_or(0.0)));
    Ok(())
}

/// Round-trip time in ms to an IPv4 address (ICMP), None if no answer.
pub fn ping(ip: &str) -> Option<u32> {
    use windows_sys::Win32::NetworkManagement::IpHelper::{IcmpCloseHandle, IcmpCreateFile, IcmpSendEcho, ICMP_ECHO_REPLY};
    let addr: Ipv4Addr = ip.parse().ok()?;
    unsafe {
        let h = IcmpCreateFile();
        if h.is_null() || h as isize == -1 {
            return None;
        }
        let data = [0u8; 32];
        let mut reply = vec![0u8; std::mem::size_of::<ICMP_ECHO_REPLY>() + data.len() + 16];
        let n = IcmpSendEcho(
            h,
            u32::from_ne_bytes(addr.octets()),
            data.as_ptr() as *const _,
            data.len() as u16,
            std::ptr::null(),
            reply.as_mut_ptr() as *mut _,
            reply.len() as u32,
            1000,
        );
        IcmpCloseHandle(h);
        if n == 0 {
            return None;
        }
        let r = &*(reply.as_ptr() as *const ICMP_ECHO_REPLY);
        (r.Status == 0).then_some(r.RoundTripTime)
    }
}
