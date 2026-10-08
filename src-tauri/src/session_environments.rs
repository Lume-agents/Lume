//! Local listeners owned by a conversation. Directory names never establish ownership.
use std::{
    collections::{HashMap, HashSet},
    net::IpAddr,
    sync::Mutex,
    thread,
    time::{Duration, Instant},
};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::{
    io::Read,
    process::{Command, Stdio},
    sync::mpsc,
};

use serde::Serialize;
#[cfg(not(target_os = "windows"))]
use sysinfo::Signal;
use sysinfo::{Pid, ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System, UpdateKind};

use crate::{domain::AgentKind, state::now_millis};

const POLL_INTERVAL: Duration = Duration::from_secs(4);
const MAX_RECORDS: usize = 256;

#[derive(Clone)]
pub(crate) struct EnvironmentOwner {
    pub session_id: String,
    pub native_id: Option<String>,
    pub agent: AgentKind,
    pub roots: Vec<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentPort {
    pub port: u16,
    pub address: String,
    pub url: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionEnvironment {
    pub id: String,
    pub session_id: String,
    pub name: String,
    pub kind: String,
    pub process_id: u32,
    pub started_at: u64,
    pub observed_at: i64,
    pub stopped_at: Option<i64>,
    pub status: String,
    pub ports: Vec<EnvironmentPort>,
    pub can_stop: bool,
}

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentSnapshot {
    pub environments: Vec<SessionEnvironment>,
    pub error: Option<String>,
}

#[derive(Default)]
struct Inventory {
    records: HashMap<String, SessionEnvironment>,
    refreshed: Option<Instant>,
    error: Option<String>,
}

#[derive(Default)]
pub struct EnvironmentMonitor(Mutex<Inventory>);

#[derive(Clone, Debug)]
struct Listener {
    pid: u32,
    address: String,
    port: u16,
}

impl EnvironmentMonitor {
    pub(crate) fn snapshot(
        &self,
        owners: &[EnvironmentOwner],
    ) -> Result<EnvironmentSnapshot, String> {
        let mut inventory = self
            .0
            .lock()
            .map_err(|_| "Environment monitor unavailable")?;
        if inventory
            .refreshed
            .is_none_or(|time| time.elapsed() >= POLL_INTERVAL)
        {
            let mut system = System::new();
            system.refresh_processes_specifics(
                ProcessesToUpdate::All,
                true,
                ProcessRefreshKind::nothing(),
            );
            match listeners(&system) {
                Ok(listeners) => {
                    let pids = listeners
                        .iter()
                        .map(|item| Pid::from_u32(item.pid))
                        .chain(
                            owners
                                .iter()
                                .flat_map(|owner| &owner.roots)
                                .map(|pid| Pid::from_u32(*pid)),
                        )
                        .collect::<HashSet<_>>();
                    let pids = pids.into_iter().collect::<Vec<_>>();
                    system.refresh_processes_specifics(
                        ProcessesToUpdate::Some(&pids),
                        true,
                        ProcessRefreshKind::nothing()
                            .with_cmd(UpdateKind::Always)
                            .with_environ(UpdateKind::Always),
                    );
                    let mut verified = owners.to_vec();
                    for owner in &mut verified {
                        owner.roots.retain(|pid| {
                            system.process(Pid::from_u32(*pid)).is_some_and(|process| {
                                let args = process
                                    .cmd()
                                    .iter()
                                    .map(|arg| arg.to_string_lossy().into_owned())
                                    .collect::<Vec<_>>();
                                crate::discovery::detect_agent_arguments(
                                    &process.name().to_string_lossy(),
                                    &args,
                                )
                                .as_ref()
                                    == Some(&owner.agent)
                            })
                        });
                    }
                    reconcile(&mut inventory.records, &system, &listeners, &verified);
                    inventory.error = None;
                }
                Err(error) => {
                    // A failed inspection is not proof that a server stopped.
                    for record in inventory
                        .records
                        .values_mut()
                        .filter(|record| record.status != "stopped")
                    {
                        record.status = "unknown".into();
                        record.can_stop = false;
                    }
                    inventory.error = Some(error);
                }
            }
            inventory.refreshed = Some(Instant::now());
        }
        let mut environments = inventory.records.values().cloned().collect::<Vec<_>>();
        let active_sessions = owners
            .iter()
            .map(|owner| owner.session_id.as_str())
            .collect::<HashSet<_>>();
        environments.retain(|item| active_sessions.contains(item.session_id.as_str()));
        environments.sort_by_key(|item| {
            (
                item.status == "stopped",
                std::cmp::Reverse(item.observed_at),
            )
        });
        Ok(EnvironmentSnapshot {
            environments,
            error: inventory.error.clone(),
        })
    }

    pub(crate) fn stop(&self, session_id: &str, environment_id: &str) -> Result<(), String> {
        let record = self
            .0
            .lock()
            .map_err(|_| "Environment monitor unavailable")?
            .records
            .get(environment_id)
            .cloned()
            .ok_or("Este ambiente não está mais disponível.")?;
        if record.session_id != session_id || !record.can_stop || record.status != "running" {
            return Err(
                "Não foi possível confirmar o processo deste ambiente. Atualize a lista.".into(),
            );
        }
        let mut system = System::new();
        let pid = Pid::from_u32(record.process_id);
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid]),
            true,
            ProcessRefreshKind::nothing(),
        );
        let process = system
            .process(pid)
            .filter(|process| process.start_time() == record.started_at)
            .ok_or("O processo terminou ou foi substituído. Atualize a lista.")?;
        // Never reuse a PID from an old observation. No agent, shell parent or editor is stopped.
        #[cfg(not(target_os = "windows"))]
        let stopped = process.kill_with(Signal::Term).unwrap_or(false);
        #[cfg(target_os = "windows")]
        let stopped = process.kill();
        if !stopped {
            return Err("O sistema recusou parar este processo.".into());
        }
        let mut exited = false;
        for _ in 0..20 {
            system.refresh_processes_specifics(
                ProcessesToUpdate::Some(&[pid]),
                true,
                ProcessRefreshKind::nothing(),
            );
            if system.process(pid).is_none_or(|process| {
                process.start_time() != record.started_at
                    || process.status() == ProcessStatus::Zombie
            }) {
                exited = true;
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
        let mut inventory = self
            .0
            .lock()
            .map_err(|_| "Environment monitor unavailable")?;
        if let Some(current) = inventory.records.get_mut(environment_id) {
            if exited {
                current.status = "stopped".into();
                current.can_stop = false;
                current.stopped_at = Some(now_millis());
            }
        }
        inventory.refreshed = None;
        if exited {
            Ok(())
        } else {
            Err("O pedido de parada foi enviado, mas o processo ainda está encerrando. Aguarde a atualização da lista.".into())
        }
    }
}

fn owner_for<'a>(
    pid: Pid,
    system: &System,
    owners: &'a [EnvironmentOwner],
) -> Option<&'a EnvironmentOwner> {
    let process = system.process(pid)?;
    // Native Codex attaches this identity to tool subprocesses, including detached servers.
    let tags = process
        .environ()
        .iter()
        .filter_map(|value| {
            let value = value.to_str()?;
            let (key, value) = value.split_once('=')?;
            matches!(key, "CODEX_THREAD_ID" | "LUME_SESSION_ID").then_some((key, value))
        })
        .collect::<Vec<_>>();
    if !tags.is_empty() {
        let matches = owners
            .iter()
            .filter(|owner| {
                tags.iter().any(|(key, value)| {
                    if *key == "CODEX_THREAD_ID" {
                        owner.agent == AgentKind::Codex
                            && owner.native_id.as_deref() == Some(*value)
                    } else {
                        owner.session_id == *value
                    }
                })
            })
            .collect::<Vec<_>>();
        // An explicit but unknown/ambiguous thread must not fall through to another chat.
        return (matches.len() == 1).then(|| matches[0]);
    }
    let mut current = pid;
    let mut visited = HashSet::new();
    for _ in 0..64 {
        if !visited.insert(current) {
            break;
        }
        let process = system.process(current)?;
        current = process.parent()?;
        let matches = owners
            .iter()
            .filter(|owner| owner.roots.contains(&current.as_u32()))
            .collect::<Vec<_>>();
        if !matches.is_empty() {
            return (matches.len() == 1).then(|| matches[0]);
        }
    }
    None
}

fn reconcile(
    records: &mut HashMap<String, SessionEnvironment>,
    system: &System,
    listeners: &[Listener],
    owners: &[EnvironmentOwner],
) {
    let mut seen = HashSet::new();
    let roots = owners
        .iter()
        .flat_map(|owner| &owner.roots)
        .copied()
        .collect::<HashSet<_>>();
    for listener in listeners {
        if roots.contains(&listener.pid) || listener.pid == std::process::id() {
            continue;
        }
        let pid = Pid::from_u32(listener.pid);
        let Some(process) = system.process(pid) else {
            continue;
        };
        if process.start_time() == 0 {
            continue;
        }
        let id = format!("env-{}-{}", listener.pid, process.start_time());
        let prior = records.get(&id);
        let owner = owner_for(pid, system, owners);
        let session_id = owner
            .map(|owner| owner.session_id.clone())
            .or_else(|| prior.map(|prior| prior.session_id.clone()));
        let Some(session_id) = session_id else {
            continue;
        };
        let (name, kind) = service_identity(&process.name().to_string_lossy(), process.cmd());
        if kind == "agent" {
            continue;
        }
        if !records.contains_key(&id) && records.len() >= MAX_RECORDS {
            let expired = records
                .iter()
                .filter(|(_, record)| record.status == "stopped")
                .min_by_key(|(_, record)| record.observed_at)
                .map(|(id, _)| id.clone());
            if let Some(expired) = expired {
                records.remove(&expired);
            } else {
                continue;
            }
        }
        let entry = records
            .entry(id.clone())
            .or_insert_with(|| SessionEnvironment {
                id: id.clone(),
                session_id,
                name,
                kind,
                process_id: listener.pid,
                started_at: process.start_time(),
                observed_at: now_millis(),
                stopped_at: None,
                status: "running".into(),
                ports: Vec::new(),
                can_stop: true,
            });
        if !seen.contains(&id) {
            entry.ports.clear();
        }
        let url = browser_url(&listener.address, listener.port, &entry.kind);
        let port = EnvironmentPort {
            port: listener.port,
            address: listener.address.clone(),
            url,
        };
        if !entry.ports.contains(&port) {
            entry.ports.push(port);
        }
        entry.ports.sort_by_key(|port| port.port);
        entry.status = "running".into();
        entry.can_stop = true;
        entry.stopped_at = None;
        seen.insert(id);
    }
    for (id, record) in records.iter_mut() {
        if !seen.contains(id) && record.status != "stopped" {
            // Loss of the listener with the same live process can mean a transient restart.
            let still_alive = system
                .process(Pid::from_u32(record.process_id))
                .is_some_and(|process| {
                    process.start_time() == record.started_at
                        && process.status() != ProcessStatus::Zombie
                });
            record.status = if still_alive { "idle" } else { "stopped" }.into();
            record.can_stop = false;
            if !still_alive && record.stopped_at.is_none() {
                record.stopped_at = Some(now_millis());
            }
        }
    }
    if records.len() > MAX_RECORDS {
        let mut expired = records
            .iter()
            .filter(|(_, record)| record.status == "stopped")
            .map(|(id, record)| (id.clone(), record.observed_at))
            .collect::<Vec<_>>();
        expired.sort_by_key(|(_, time)| *time);
        for (id, _) in expired
            .into_iter()
            .take(records.len().saturating_sub(MAX_RECORDS))
        {
            records.remove(&id);
        }
    }
}

fn service_identity(name: &str, args: &[std::ffi::OsString]) -> (String, String) {
    let lower = name.trim_end_matches(".exe").to_lowercase();
    let command = args
        .iter()
        .filter_map(|arg| arg.to_str())
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    if matches!(
        lower.as_str(),
        "codex" | "claude" | "opencode" | "agy" | "gemini" | "lume"
    ) || crate::discovery::detect_agent_arguments(
        name,
        &args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
    )
    .is_some()
    {
        return (name.into(), "agent".into());
    }
    let label = if command.contains("vite") {
        Some("Vite")
    } else if command.contains("next") {
        Some("Next.js")
    } else if command.contains("nuxt") {
        Some("Nuxt")
    } else if command.contains("http.server") {
        Some("Python HTTP")
    } else if command.contains("uvicorn") {
        Some("Uvicorn")
    } else if command.contains("gunicorn") {
        Some("Gunicorn")
    } else if lower == "dotnet" {
        Some(".NET")
    } else {
        None
    };
    let database = matches!(
        lower.as_str(),
        "postgres" | "mysqld" | "mariadbd" | "redis-server" | "mongod"
    );
    (
        label.unwrap_or(name).into(),
        if database {
            "database"
        } else if label.is_some() {
            "web"
        } else {
            "service"
        }
        .into(),
    )
}

fn browser_url(address: &str, port: u16, kind: &str) -> Option<String> {
    if kind != "web" {
        return None;
    }
    let address = address.trim_matches(['[', ']']);
    let ip = if address == "*" {
        "127.0.0.1".parse().ok()?
    } else {
        address.parse::<IpAddr>().ok()?
    };
    // Only offer a local browser destination, never an external bind interface.
    if !ip.is_loopback() && !ip.is_unspecified() {
        return None;
    }
    let host = if ip.is_ipv6() { "[::1]" } else { "127.0.0.1" };
    Some(format!("http://{host}:{port}"))
}

#[cfg(any(target_os = "linux", target_os = "macos", test))]
fn split_endpoint(value: &str) -> Option<(String, u16)> {
    let (address, port) = value.rsplit_once(':')?;
    let port = port.parse::<u16>().ok()?;
    (port != 0).then(|| (address.trim_matches(['[', ']']).into(), port))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn bounded_output(command: &mut Command, empty_exit_is_no_matches: bool) -> Result<String, String> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| "Não foi possível consultar as portas locais.".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or("Não foi possível consultar as portas locais.")?;
    let stderr = child
        .stderr
        .take()
        .ok_or("Não foi possível consultar as portas locais.")?;
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout.take(512 * 1024 + 1).read_to_end(&mut bytes);
        let _ = sender.send((result, bytes));
    });
    let (error_sender, error_receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stderr.take(8193).read_to_end(&mut bytes);
        let _ = error_sender.send((result, bytes));
    });
    let deadline = Instant::now() + Duration::from_secs(2);
    let status = loop {
        if let Ok(Some(status)) = child.try_wait() {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("A consulta de portas locais demorou demais.".into());
        }
        thread::sleep(Duration::from_millis(10));
    };
    let (read, bytes) = receiver
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .map_err(|_| "A consulta de portas locais demorou demais.")?;
    read.map_err(|_| "Não foi possível ler as portas locais.")?;
    let (error_read, error_bytes) = error_receiver
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .map_err(|_| "A consulta de portas locais demorou demais.")?;
    error_read.map_err(|_| "Não foi possível ler as portas locais.")?;
    let no_matches = empty_exit_is_no_matches
        && status.code() == Some(1)
        && bytes.is_empty()
        && error_bytes.is_empty();
    if !status.success() && !no_matches {
        return Err("A ferramenta do sistema não conseguiu consultar as portas locais.".into());
    }
    if bytes.len() > 512 * 1024 {
        return Err("A lista de portas excedeu o limite de leitura.".into());
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(target_os = "linux")]
fn listeners(system: &System) -> Result<Vec<Listener>, String> {
    let ss = ["/usr/bin/ss", "/bin/ss"]
        .into_iter()
        .find(|path| std::path::Path::new(path).is_file());
    if let Some(ss) = ss {
        let output = bounded_output(Command::new(ss).args(["-ltnpH"]), false)?;
        Ok(parse_ss(&output))
    } else {
        proc_listeners(system)
    }
}

#[cfg(any(target_os = "linux", test))]
fn parse_ss(output: &str) -> Vec<Listener> {
    output
        .lines()
        .flat_map(|line| {
            let endpoint = line.split_whitespace().nth(3).and_then(split_endpoint);
            let mut result = Vec::new();
            if let Some((address, port)) = endpoint {
                for tail in line.split("pid=").skip(1) {
                    if let Ok(pid) = tail
                        .chars()
                        .take_while(char::is_ascii_digit)
                        .collect::<String>()
                        .parse::<u32>()
                    {
                        result.push(Listener {
                            pid,
                            address: address.clone(),
                            port,
                        });
                    }
                }
            }
            result
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn proc_listeners(system: &System) -> Result<Vec<Listener>, String> {
    let mut sockets = HashMap::new();
    for path in ["/proc/net/tcp", "/proc/net/tcp6"] {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        for line in text.lines().skip(1) {
            let columns = line.split_whitespace().collect::<Vec<_>>();
            if columns.len() < 10 || columns[3] != "0A" {
                continue;
            }
            let Some((address, port)) = columns[1].split_once(':') else {
                continue;
            };
            let Ok(port) = u16::from_str_radix(port, 16) else {
                continue;
            };
            let address = match address {
                "00000000" => "0.0.0.0",
                "0100007F" => "127.0.0.1",
                "00000000000000000000000000000000" => "::",
                "00000000000000000000000001000000" => "::1",
                _ => "private-interface",
            };
            sockets.insert(columns[9].to_string(), (address.to_string(), port));
        }
    }
    if !std::path::Path::new("/proc/net/tcp").exists() {
        return Err("As portas locais não estão disponíveis neste ambiente.".into());
    }
    let mut result = Vec::new();
    for pid in system.processes().keys() {
        let Ok(files) = std::fs::read_dir(format!("/proc/{pid}/fd")) else {
            continue;
        };
        for file in files.filter_map(Result::ok).take(1024) {
            let Ok(target) = std::fs::read_link(file.path()) else {
                continue;
            };
            let target = target.to_string_lossy();
            let Some(inode) = target
                .strip_prefix("socket:[")
                .and_then(|value| value.strip_suffix(']'))
            else {
                continue;
            };
            if let Some((address, port)) = sockets.get(inode) {
                result.push(Listener {
                    pid: pid.as_u32(),
                    address: address.clone(),
                    port: *port,
                });
            }
        }
    }
    Ok(result)
}

#[cfg(target_os = "windows")]
fn listeners(_: &System) -> Result<Vec<Listener>, String> {
    use windows_sys::Win32::{
        Foundation::{ERROR_INSUFFICIENT_BUFFER, NO_ERROR},
        NetworkManagement::IpHelper::{GetExtendedTcpTable, TCP_TABLE_OWNER_PID_LISTENER},
        Networking::WinSock::{AF_INET, AF_INET6},
    };
    let mut result = Vec::new();
    for (family, ipv6) in [(AF_INET, false), (AF_INET6, true)] {
        let mut size = 0;
        // SAFETY: size is a valid out pointer; a null table requests its size only.
        let status = unsafe {
            GetExtendedTcpTable(
                std::ptr::null_mut(),
                &mut size,
                0,
                family as u32,
                TCP_TABLE_OWNER_PID_LISTENER,
                0,
            )
        };
        if status != ERROR_INSUFFICIENT_BUFFER && status != NO_ERROR {
            return Err("O Windows não conseguiu consultar as portas locais.".into());
        }
        if size == 0 {
            continue;
        }
        let mut complete = false;
        for _ in 0..3 {
            if size < 4 || size > 512 * 1024 {
                return Err("A lista de portas excedeu o limite de leitura.".into());
            }
            let mut buffer = vec![0_u32; (size as usize).div_ceil(4)];
            // SAFETY: DWORD-aligned buffer owns at least size writable bytes.
            let status = unsafe {
                GetExtendedTcpTable(
                    buffer.as_mut_ptr().cast(),
                    &mut size,
                    0,
                    family as u32,
                    TCP_TABLE_OWNER_PID_LISTENER,
                    0,
                )
            };
            if status == ERROR_INSUFFICIENT_BUFFER {
                continue;
            }
            if status != NO_ERROR {
                return Err("O Windows não conseguiu consultar as portas locais.".into());
            }
            let bytes = buffer
                .into_iter()
                .flat_map(u32::to_ne_bytes)
                .collect::<Vec<_>>();
            result.extend(parse_windows_table(&bytes, ipv6)?);
            complete = true;
            break;
        }
        if !complete {
            return Err("A lista de portas mudou durante a consulta. Tente novamente.".into());
        }
    }
    Ok(result)
}

#[cfg(any(target_os = "windows", test))]
fn parse_windows_table(bytes: &[u8], ipv6: bool) -> Result<Vec<Listener>, String> {
    // OWNER_PID tables contain a DWORD count followed by 24/56-byte IPv4/IPv6 rows.
    let count = u32::from_le_bytes(
        bytes
            .get(..4)
            .ok_or("Tabela de portas incompleta.")?
            .try_into()
            .unwrap(),
    ) as usize;
    let row_size = if ipv6 { 56 } else { 24 };
    if count > bytes.len().saturating_sub(4) / row_size {
        return Err("Tabela de portas incompleta.".into());
    }
    let mut result = Vec::new();
    for row in bytes[4..].chunks_exact(row_size).take(count) {
        let (address, port_offset, pid_offset) = if ipv6 {
            (
                std::net::Ipv6Addr::from(<[u8; 16]>::try_from(&row[..16]).unwrap()).to_string(),
                20,
                52,
            )
        } else {
            (
                std::net::Ipv4Addr::from(<[u8; 4]>::try_from(&row[4..8]).unwrap()).to_string(),
                8,
                20,
            )
        };
        result.push(Listener {
            pid: u32::from_le_bytes(row[pid_offset..pid_offset + 4].try_into().unwrap()),
            address,
            port: u16::from_be_bytes(row[port_offset..port_offset + 2].try_into().unwrap()),
        });
    }
    Ok(result)
}

#[cfg(target_os = "macos")]
fn listeners(_: &System) -> Result<Vec<Listener>, String> {
    let output = bounded_output(
        Command::new("/usr/sbin/lsof").args(["-nP", "-iTCP", "-sTCP:LISTEN", "-Fpn"]),
        true,
    )?;
    Ok(parse_lsof(&output))
}

#[cfg(any(target_os = "macos", test))]
fn parse_lsof(output: &str) -> Vec<Listener> {
    let mut pid = None;
    output
        .lines()
        .filter_map(|line| {
            if let Some(value) = line.strip_prefix('p') {
                pid = value.parse().ok();
                return None;
            }
            let (address, port) = split_endpoint(line.strip_prefix('n')?)?;
            Some(Listener {
                pid: pid?,
                address,
                port,
            })
        })
        .collect()
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
fn listeners(_: &System) -> Result<Vec<Listener>, String> {
    Err("Monitoramento de ambientes não disponível nesta plataforma.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn an_empty_lsof_result_is_distinct_from_a_failed_probe() {
        assert_eq!(
            bounded_output(Command::new("/bin/sh").args(["-c", "exit 1"]), true).unwrap(),
            ""
        );
        assert!(bounded_output(Command::new("/bin/sh").args(["-c", "exit 1"]), false).is_err());
        assert!(bounded_output(
            Command::new("/bin/sh").args(["-c", "printf problem >&2; exit 1"]),
            true
        )
        .is_err());
        assert!(bounded_output(Command::new("/bin/sh").args(["-c", "exit 2"]), true).is_err());
    }

    #[test]
    fn platform_parsers_ignore_connections_and_keep_ipv6_and_multiple_owners() {
        let linux = parse_ss("LISTEN 0 128 [::1]:5173 [::]:* users:((\"node\",pid=234,fd=9),(\"node\",pid=235,fd=9))\nLISTEN 0 128 0.0.0.0:8080 0.0.0.0:*");
        assert_eq!(linux.len(), 2);
        assert_eq!(
            (linux[0].pid, linux[0].port, linux[0].address.as_str()),
            (234, 5173, "::1")
        );
        let mut windows = vec![0_u8; 28];
        windows[..4].copy_from_slice(&1_u32.to_le_bytes());
        windows[8..12].copy_from_slice(&[127, 0, 0, 1]);
        windows[12..14].copy_from_slice(&3000_u16.to_be_bytes());
        windows[24..28].copy_from_slice(&42_u32.to_le_bytes());
        let row = &parse_windows_table(&windows, false).unwrap()[0];
        assert_eq!(
            (row.address.as_str(), row.port, row.pid),
            ("127.0.0.1", 3000, 42)
        );
        assert!(parse_windows_table(&windows[..27], false).is_err());
        let mut ipv6 = vec![0_u8; 60];
        ipv6[..4].copy_from_slice(&1_u32.to_le_bytes());
        ipv6[19] = 1;
        ipv6[24..26].copy_from_slice(&5000_u16.to_be_bytes());
        ipv6[56..60].copy_from_slice(&43_u32.to_le_bytes());
        let row = &parse_windows_table(&ipv6, true).unwrap()[0];
        assert_eq!((row.address.as_str(), row.port, row.pid), ("::1", 5000, 43));
        let mac = parse_lsof("p42\nn*:8000\np43\nn[::1]:5000\n");
        assert_eq!(mac.len(), 2);
        assert_eq!(mac[1].pid, 43);
    }

    #[test]
    fn only_http_services_offer_loopback_browser_links() {
        assert_eq!(
            browser_url("0.0.0.0", 5173, "web").as_deref(),
            Some("http://127.0.0.1:5173")
        );
        assert_eq!(
            browser_url("::", 5173, "web").as_deref(),
            Some("http://[::1]:5173")
        );
        assert!(browser_url("127.0.0.1", 5432, "database").is_none());
        assert!(browser_url("10.0.0.4", 5173, "web").is_none());
    }

    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "fixture subprocess for the local listener smoke test"]
    fn fixture_listener() {
        use std::io::Write;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("fixture listener");
        println!("LUME_ENV_READY {}", listener.local_addr().unwrap().port());
        std::io::stdout().flush().unwrap();
        loop {
            let _ = listener.accept();
        }
    }

    #[cfg(target_os = "linux")]
    struct Fixture(std::process::Child);
    #[cfg(target_os = "linux")]
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    #[cfg(target_os = "linux")]
    fn spawn_fixture(thread_id: Option<&str>) -> (Fixture, u16) {
        use std::io::{BufRead, BufReader};
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--ignored",
                "--exact",
                "session_environments::tests::fixture_listener",
                "--nocapture",
            ])
            .env_remove("CODEX_THREAD_ID")
            .env_remove("LUME_SESSION_ID")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        if let Some(id) = thread_id {
            command.env("CODEX_THREAD_ID", id);
        }
        let mut fixture = Fixture(command.spawn().expect("start fixture"));
        let stdout = fixture.0.stdout.take().unwrap();
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Some(port) = line.trim().strip_prefix("LUME_ENV_READY ") {
                    let _ = sender.send(port.parse::<u16>().unwrap());
                    break;
                }
            }
        });
        let port = receiver
            .recv_timeout(Duration::from_secs(5))
            .expect("fixture ready");
        (fixture, port)
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn live_listener_is_owned_by_thread_and_stopping_preserves_other_processes() {
        let owners = vec![EnvironmentOwner {
            session_id: "fixture-chat".into(),
            native_id: Some("fixture-thread".into()),
            agent: AgentKind::Codex,
            roots: vec![],
        }];
        let (mut owned, port) = spawn_fixture(Some("fixture-thread"));
        let (mut unrelated, _) = spawn_fixture(None);
        let monitor = EnvironmentMonitor::default();
        let snapshot = monitor.snapshot(&owners).unwrap();
        assert_eq!(
            snapshot.environments.len(),
            1,
            "unrelated listeners must not be assigned"
        );
        let record = &snapshot.environments[0];
        assert_eq!(record.process_id, owned.0.id());
        assert!(record.ports.iter().any(|item| item.port == port));
        assert!(monitor.stop("another-chat", &record.id).is_err());
        assert!(owned.0.try_wait().unwrap().is_none());
        // Simulate a recycled PID: an old start marker may never stop a new process.
        monitor
            .0
            .lock()
            .unwrap()
            .records
            .get_mut(&record.id)
            .unwrap()
            .started_at -= 1;
        assert!(monitor.stop("fixture-chat", &record.id).is_err());
        assert!(owned.0.try_wait().unwrap().is_none());
        monitor
            .0
            .lock()
            .unwrap()
            .records
            .get_mut(&record.id)
            .unwrap()
            .started_at = record.started_at;
        monitor.stop("fixture-chat", &record.id).unwrap();
        for _ in 0..30 {
            if owned.0.try_wait().unwrap().is_some() {
                break;
            }
            thread::sleep(Duration::from_millis(30));
        }
        assert!(owned.0.try_wait().unwrap().is_some());
        assert!(
            unrelated.0.try_wait().unwrap().is_none(),
            "another environment must survive"
        );
        let snapshot = monitor.snapshot(&owners).unwrap();
        assert_eq!(snapshot.environments[0].status, "stopped");
        assert!(!snapshot.environments[0].can_stop);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn ambiguous_threads_and_agent_listeners_are_not_assigned() {
        let (_fixture, _) = spawn_fixture(Some("ambiguous-thread"));
        let owner = EnvironmentOwner {
            session_id: "first".into(),
            native_id: Some("ambiguous-thread".into()),
            agent: AgentKind::Codex,
            roots: vec![],
        };
        let mut duplicate = owner.clone();
        duplicate.session_id = "second".into();
        assert!(EnvironmentMonitor::default()
            .snapshot(&[owner, duplicate])
            .unwrap()
            .environments
            .is_empty());
        assert_eq!(
            service_identity("codex", &["codex".into(), "app-server".into()]).1,
            "agent"
        );
    }
}
