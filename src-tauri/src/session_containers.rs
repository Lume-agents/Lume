//! Read-only container inventory, associated by recorded Docker invocations, never by directory alone.
use crate::{
    domain::AgentKind,
    session_environments::{EnvironmentOwner, EnvironmentPort, SessionEnvironment},
    state::now_millis,
};
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Selector {
    Container(String),
    Compose {
        project: Option<String>,
        files: Vec<String>,
        directory: Option<String>,
    },
}

type Claims = HashMap<(AgentKind, String), Vec<Selector>>;
fn claims() -> &'static Mutex<Claims> {
    static CLAIMS: OnceLock<Mutex<Claims>> = OnceLock::new();
    CLAIMS.get_or_init(Default::default)
}

pub(crate) fn observe_input(
    agent: AgentKind,
    thread: &str,
    input: &Value,
    directory: Option<&str>,
) {
    if let Some(command) = input
        .get("cmd")
        .or_else(|| input.get("command"))
        .and_then(Value::as_str)
    {
        remember(
            agent,
            thread,
            command,
            input
                .get("workdir")
                .or_else(|| input.get("cwd"))
                .and_then(Value::as_str)
                .or(directory),
        );
    } else if let Some(source) = input.as_str().filter(|source| source.contains("docker")) {
        if source.len() > 128 * 1024 {
            return;
        }
        if source.trim_start().starts_with('{') {
            if let Ok(value @ Value::Object(_)) = serde_json::from_str::<Value>(source) {
                observe_input(agent, thread, &value, directory);
                return;
            }
        }
        for (_, argument) in
            crate::codex_sessions::nested_tool_calls(source, &["exec_command", "shell", "terminal"])
        {
            if let Some(command) =
                literal_field(&argument, "cmd").or_else(|| literal_field(&argument, "command"))
            {
                let cwd =
                    literal_field(&argument, "workdir").or_else(|| literal_field(&argument, "cwd"));
                remember(
                    agent.clone(),
                    thread,
                    &command,
                    cwd.as_deref().or(directory),
                );
            }
        }
        // Native exec_command input can itself be a plain shell command.
        remember(
            agent,
            thread,
            source.split("\n\n").next().unwrap_or(""),
            directory,
        );
    }
}

fn remember(agent: AgentKind, thread: &str, source: &str, directory: Option<&str>) {
    if source.len() > 128 * 1024 || !source.contains("docker") {
        return;
    }
    let parsed = selectors(source, directory, 0);
    if parsed.is_empty() {
        return;
    }
    let Ok(mut registry) = claims().lock() else {
        return;
    };
    if registry.len() >= 256 && !registry.contains_key(&(agent.clone(), thread.into())) {
        return;
    }
    let stored = registry.entry((agent, thread.into())).or_default();
    for selector in parsed {
        if stored.len() < 128 && !stored.contains(&selector) {
            stored.push(selector);
        }
    }
}

/// Decode literal fields only. Dynamic JS, templates and expressions are deliberately not evaluated.
fn literal_field(object: &str, field: &str) -> Option<String> {
    if let Ok(value) = serde_json::from_str::<Value>(object) {
        return value.get(field)?.as_str().map(str::to_owned);
    }
    let bytes = object.as_bytes();
    let mut index = 0;
    let mut depth = 0i32;
    while index < bytes.len() {
        let c = bytes[index];
        if c == b'/' && bytes.get(index + 1) == Some(&b'/') {
            while bytes.get(index).is_some_and(|c| *c != b'\n') {
                index += 1;
            }
            continue;
        }
        if c == b'/' && bytes.get(index + 1) == Some(&b'*') {
            index += 2;
            while index + 1 < bytes.len() && &bytes[index..index + 2] != b"*/" {
                index += 1;
            }
            index += 2;
            continue;
        }
        if matches!(c, b'{' | b'[' | b'(') {
            depth += 1;
            index += 1;
            continue;
        }
        if matches!(c, b'}' | b']' | b')') {
            depth -= 1;
            index += 1;
            continue;
        }
        if matches!(c, b'\'' | b'"' | b'`') {
            let (end, value) = js_string(object, index)?;
            index = end;
            if depth != 1 || value.as_deref() != Some(field) {
                continue;
            }
        } else if depth == 1
            && (c.is_ascii_alphabetic() || c == b'_')
            && (index == 0
                || !(bytes[index - 1].is_ascii_alphanumeric()
                    || matches!(bytes[index - 1], b'_' | b'$')
                    || bytes[index - 1] >= 128))
        {
            let start = index;
            while bytes
                .get(index)
                .is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_')
            {
                index += 1;
            }
            if &object[start..index] != field {
                continue;
            }
        } else {
            index += 1;
            continue;
        }
        while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
            index += 1;
        }
        if bytes.get(index) != Some(&b':') {
            continue;
        }
        index += 1;
        while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
            index += 1;
        }
        let (end, value) = js_string(object, index)?;
        let after = object[end..].trim_start();
        return (after.starts_with(',') || after.starts_with('}'))
            .then_some(value)
            .flatten();
    }
    None
}

fn js_string(source: &str, start: usize) -> Option<(usize, Option<String>)> {
    let bytes = source.as_bytes();
    let quote = *bytes.get(start)?;
    if !matches!(quote, b'\'' | b'"' | b'`') {
        return None;
    }
    let mut end = start + 1;
    let mut escaped = false;
    while end < bytes.len() {
        if escaped {
            escaped = false;
        } else if bytes[end] == b'\\' {
            escaped = true;
        } else if bytes[end] == quote {
            let raw = &source[start..=end];
            let value = if quote == b'"' {
                serde_json::from_str::<String>(raw).ok()
            } else if quote == b'\'' && !raw.contains('\\') {
                Some(raw[1..raw.len() - 1].to_owned())
            } else {
                None
            };
            return Some((end + 1, value));
        }
        end += 1;
    }
    None
}

fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))
}

/// A path as the agent's shell wrote it: `/srv/app` or `C:/app`. It is read as text, not through `Path`, so it
/// means the same on every platform (on Windows `/srv/app` is not "absolute" to `Path`, yet it is a Linux path).
fn absolute_root(value: &str) -> Option<(&str, &str)> {
    if let Some(rest) = value.strip_prefix('/') {
        return Some(("/", rest));
    }
    let bytes = value.as_bytes();
    (bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'/')
        .then(|| value.split_at(3))
}

fn clean_path(value: &str, directory: Option<&str>) -> Option<String> {
    if value.contains('$') || value.contains('`') {
        return None;
    }
    let value = value.replace('\\', "/");
    let joined = if absolute_root(&value).is_some() {
        value
    } else {
        format!("{}/{}", directory?.replace('\\', "/").trim_end_matches('/'), value)
    };
    let (root, rest) = absolute_root(&joined)?;
    let mut parts: Vec<&str> = Vec::new();
    for part in rest.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    Some(format!("{root}{}", parts.join("/")).trim_end_matches('/').to_owned())
}

/// Shell separators outside quotes, not occurrences of "docker" inside echo/cat/log output.
fn segments(source: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut quote = None;
    let mut escaped = false;
    let mut start = 0;
    for (index, c) in source.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if c == '\\' && quote != Some('\'') {
            escaped = true;
            continue;
        }
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            continue;
        }
        if matches!(c, '\'' | '"' | '`') {
            quote = Some(c);
            continue;
        }
        if matches!(c, ';' | '\n' | '&' | '|') {
            parts.push(&source[start..index]);
            start = index + c.len_utf8();
        }
    }
    if quote.is_none() {
        parts.push(&source[start..]);
    }
    parts
}

fn selectors(source: &str, directory: Option<&str>, depth: u8) -> Vec<Selector> {
    if depth > 2 {
        return Vec::new();
    }
    let mut cwd = directory.map(str::to_owned);
    let mut result = Vec::new();
    // Here-documents and substitutions can contain code that is not being executed here.
    if source.contains("<<") || source.contains("$(") || source.contains('`') {
        return result;
    }
    for segment in segments(source) {
        let Ok(words) = shell_words::split(segment) else {
            continue;
        };
        let Some(executable) = words
            .first()
            .map(|word| word.rsplit(['/', '\\']).next().unwrap_or(word))
        else {
            continue;
        };
        if executable == "cd" && words.len() == 2 {
            cwd = clean_path(&words[1], cwd.as_deref());
            continue;
        }
        if matches!(executable, "bash" | "sh" | "zsh" | "pwsh" | "powershell")
            && words.len() == 3
            && matches!(words[1].as_str(), "-c" | "-lc" | "-Command")
        {
            result.extend(selectors(&words[2], cwd.as_deref(), depth + 1));
            continue;
        }
        if !matches!(
            executable,
            "docker" | "docker.exe" | "docker-compose" | "docker-compose.exe"
        ) {
            continue;
        }
        let compose = executable.starts_with("docker-compose")
            || words.get(1).map(String::as_str) == Some("compose");
        let mut i = if executable.starts_with("docker-compose") {
            1
        } else if compose {
            2
        } else {
            1
        };
        if compose {
            let mut project = None;
            let mut files = Vec::new();
            let mut valid = true;
            while let Some(flag) = words.get(i) {
                if matches!(flag.as_str(), "up" | "start" | "restart") {
                    break;
                }
                let (key, inline) = flag
                    .split_once('=')
                    .map(|(a, b)| (a, Some(b)))
                    .unwrap_or((flag, None));
                if !matches!(
                    key,
                    "-p" | "--project-name"
                        | "-f"
                        | "--file"
                        | "--project-directory"
                        | "--env-file"
                        | "--profile"
                ) {
                    valid = false;
                    break;
                }
                let Some(value) = inline.or_else(|| words.get(i + 1).map(String::as_str)) else {
                    valid = false;
                    break;
                };
                match key {
                    "-p" | "--project-name" => {
                        if valid_name(value) {
                            project = Some(value.into());
                        } else {
                            valid = false;
                        }
                    }
                    "-f" | "--file" => {
                        if let Some(file) = clean_path(value, cwd.as_deref()) {
                            files.push(file);
                        } else {
                            valid = false;
                        }
                    }
                    "--project-directory" => {
                        cwd = clean_path(value, cwd.as_deref());
                        if cwd.is_none() {
                            valid = false;
                        }
                    }
                    _ => {}
                }
                i += if inline.is_some() { 1 } else { 2 };
            }
            if valid
                && words
                    .get(i)
                    .is_some_and(|action| matches!(action.as_str(), "up" | "start" | "restart"))
                && (project.is_some() || !files.is_empty() || cwd.is_some())
            {
                result.push(Selector::Compose {
                    project,
                    files,
                    directory: cwd.clone(),
                });
            }
            continue;
        }
        if words.get(i).map(String::as_str) == Some("container") {
            i += 1;
        }
        match words.get(i).map(String::as_str) {
            Some("run" | "create") => {
                i += 1;
                while let Some(flag) = words.get(i) {
                    if !flag.starts_with('-') {
                        break;
                    }
                    let (key, inline) = flag
                        .split_once('=')
                        .map(|(a, b)| (a, Some(b)))
                        .unwrap_or((flag, None));
                    if matches!(
                        key,
                        "--name"
                            | "-e"
                            | "--env"
                            | "--env-file"
                            | "-v"
                            | "--volume"
                            | "--mount"
                            | "-p"
                            | "--publish"
                            | "--network"
                            | "-w"
                            | "--workdir"
                            | "-u"
                            | "--user"
                            | "--restart"
                            | "--entrypoint"
                            | "--label"
                            | "-l"
                            | "--add-host"
                            | "--platform"
                            | "--pull"
                            | "--hostname"
                            | "--memory"
                            | "--cpus"
                    ) {
                        let Some(value) = inline.or_else(|| words.get(i + 1).map(String::as_str))
                        else {
                            break;
                        };
                        if key == "--name" && valid_name(value) {
                            result.push(Selector::Container(value.into()));
                        }
                        i += if inline.is_some() { 1 } else { 2 };
                    } else if matches!(
                        key,
                        "-d" | "--detach"
                            | "--rm"
                            | "--init"
                            | "-it"
                            | "-i"
                            | "-t"
                            | "--privileged"
                            | "--read-only"
                    ) {
                        i += 1;
                    } else {
                        break;
                    }
                }
            }
            // Exec is explicit use of an existing environment, not proof that this chat created it.
            // Containers are therefore read-only in Lume even when this evidence is unambiguous.
            Some("start" | "restart" | "exec") => {
                let exec = words[i] == "exec";
                i += 1;
                while let Some(word) = words.get(i) {
                    if matches!(
                        word.as_str(),
                        "-d" | "-i" | "--detach" | "--interactive" | "-a" | "--attach"
                    ) || (exec && matches!(word.as_str(), "-t" | "-it" | "--tty"))
                    {
                        i += 1;
                        continue;
                    }
                    if matches!(
                        word.as_str(),
                        "-e" | "--env" | "-w" | "--workdir" | "-u" | "--user" | "-t" | "--time"
                    ) {
                        i += 2;
                        continue;
                    }
                    if !valid_name(word) {
                        break;
                    }
                    result.push(Selector::Container(word.clone()));
                    i += 1;
                    if exec {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    result
}

#[derive(Clone)]
struct Container {
    id: String,
    name: String,
    image: String,
    project: Option<String>,
    directory: Option<String>,
    files: Vec<String>,
    started: u64,
    ports: Vec<EnvironmentPort>,
}

fn decode(output: &str) -> Vec<Container> {
    output
        .lines()
        .filter_map(|line| {
            let v: Value = serde_json::from_str(line).ok()?;
            let id = v["id"].as_str()?;
            if id.len() != 64 || !id.bytes().all(|c| c.is_ascii_hexdigit()) || v["running"] != true
            {
                return None;
            }
            let started = chrono::DateTime::parse_from_rfc3339(v["started"].as_str()?)
                .ok()?
                .timestamp()
                .max(0) as u64;
            let mut ports = Vec::new();
            for (key, bindings) in v["ports"].as_object().into_iter().flatten() {
                if !key.ends_with("/tcp") {
                    continue;
                }
                for binding in bindings.as_array().into_iter().flatten() {
                    let port = binding["HostPort"].as_str()?.parse::<u16>().ok()?;
                    let address = binding["HostIp"].as_str()?.to_owned();
                    ports.push(EnvironmentPort {
                        port,
                        address,
                        url: None,
                    });
                }
            }
            ports.sort_by_key(|p| p.port);
            ports.dedup();
            Some(Container {
                id: id.into(),
                name: v["name"].as_str()?.trim_start_matches('/').into(),
                image: v["image"].as_str().unwrap_or("").into(),
                project: v["project"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned),
                directory: v["directory"].as_str().and_then(|s| clean_path(s, None)),
                files: v["files"]
                    .as_str()
                    .unwrap_or("")
                    .split(',')
                    .filter_map(|s| clean_path(s, None))
                    .collect(),
                started,
                ports,
            })
        })
        .collect()
}

fn matches(container: &Container, selector: &Selector) -> bool {
    match selector {
        Selector::Container(name) => {
            container.name == *name || (name.len() >= 12 && container.id.starts_with(name))
        }
        Selector::Compose {
            project,
            files,
            directory,
        } => {
            if container.project.is_none() {
                return false;
            }
            if let Some(project) = project {
                if container.project.as_ref() != Some(project) {
                    return false;
                }
            }
            if !files.is_empty() {
                return files.iter().all(|file| container.files.contains(file));
            }
            project.is_some()
                || directory
                    .as_ref()
                    .is_some_and(|dir| container.directory.as_ref() == Some(dir))
        }
    }
}

#[derive(Default)]
pub(crate) struct ContainerInventory {
    refreshed: Option<Instant>,
    containers: Vec<Container>,
    error: Option<String>,
}

impl ContainerInventory {
    pub(crate) fn snapshot(
        &mut self,
        owners: &[EnvironmentOwner],
    ) -> Result<Vec<SessionEnvironment>, String> {
        for owner in owners {
            let key = owner.native_id.as_deref().unwrap_or(&owner.session_id);
            for command in &owner.container_commands {
                observe_input(
                    owner.agent.clone(),
                    key,
                    &Value::String(command.clone()),
                    owner.working_directory.as_deref(),
                );
            }
        }
        let candidates: Vec<_> = {
            let Ok(registry) = claims().lock() else {
                return Ok(Vec::new());
            };
            owners
                .iter()
                .filter_map(|owner| {
                    registry
                        .get(&(
                            owner.agent.clone(),
                            owner
                                .native_id
                                .clone()
                                .unwrap_or_else(|| owner.session_id.clone()),
                        ))
                        .map(|claims| (owner, claims.clone()))
                })
                .collect()
        };
        if candidates.is_empty() {
            return Ok(Vec::new());
        }
        if self
            .refreshed
            .is_none_or(|at| at.elapsed() >= Duration::from_secs(12))
        {
            self.refreshed = Some(Instant::now());
            match inspect() {
                Ok(containers) => {
                    self.containers = containers;
                    self.error = None;
                }
                Err(error) => {
                    self.error = Some(error);
                }
            }
        }
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        let mut result = Vec::new();
        for container in &self.containers {
            let matched: Vec<_> = candidates
                .iter()
                .filter(|(_, selectors)| selectors.iter().any(|s| matches(container, s)))
                .collect();
            if matched.len() != 1 {
                continue;
            }
            let image = container
                .image
                .rsplit('/')
                .next()
                .unwrap_or("")
                .split([':', '@'])
                .next()
                .unwrap_or("");
            let mut kind = if matches!(
                image,
                "postgres"
                    | "postgresql"
                    | "mysql"
                    | "mariadb"
                    | "redis"
                    | "mongo"
                    | "mongodb"
                    | "mssql-server"
            ) {
                "database"
            } else {
                "service"
            }
            .to_owned();
            let mut ports = container.ports.clone();
            for port in &mut ports {
                let kind = if kind == "database" {
                    "database"
                } else if matches!(
                    port.port,
                    80 | 443
                        | 3000
                        | 4200
                        | 5173
                        | 8000
                        | 8010
                        | 8080
                        | 8081
                        | 8082
                        | 15672
                        | 15673
                ) {
                    "web"
                } else {
                    "service"
                };
                port.url = crate::session_environments::browser_url(&port.address, port.port, kind);
            }
            if kind != "database" && ports.iter().any(|port| port.url.is_some()) {
                kind = "web".into();
            }
            result.push(SessionEnvironment {
                id: format!("docker-{}", container.id),
                session_id: matched[0].0.session_id.clone(),
                name: container.name.clone(),
                kind,
                process_id: 0,
                started_at: container.started,
                observed_at: now_millis(),
                stopped_at: None,
                status: "running".into(),
                ports,
                can_stop: false,
            });
        }
        Ok(result)
    }
}

fn inspect() -> Result<Vec<Container>, String> {
    if !crate::executables::available("docker") {
        return Ok(Vec::new());
    }
    // Reject remote daemons: these are local environments, and remote PIDs are not host PIDs.
    let endpoint = if std::env::var_os("DOCKER_CONTEXT").is_none() {
        std::env::var("DOCKER_HOST").ok()
    } else {
        None
    };
    let endpoint = match endpoint {
        Some(endpoint) => endpoint,
        None => {
            let mut command = crate::executables::command("docker")?;
            command.args([
                "context",
                "inspect",
                "--format",
                "{{.Endpoints.docker.Host}}",
            ]);
            crate::session_environments::bounded_output(&mut command, false)?
                .trim()
                .to_owned()
        }
    };
    if !(endpoint.starts_with("unix:///") || endpoint.starts_with("npipe:////./pipe/")) {
        return Ok(Vec::new());
    }
    let mut list = crate::executables::command("docker")?;
    list.env_remove("DOCKER_CONTEXT")
        .env_remove("DOCKER_HOST")
        .args(["--host", &endpoint, "ps", "--quiet", "--no-trunc"]);
    let ids = crate::session_environments::bounded_output(&mut list, false)?;
    let ids: Vec<_> = ids
        .lines()
        .filter(|id| id.len() == 64 && id.bytes().all(|c| c.is_ascii_hexdigit()))
        .take(128)
        .collect();
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    // Project labels and public port mappings only: never emit Config.Env, mounts or credentials.
    const FORMAT: &str = r#"{"id":{{json .Id}},"name":{{json .Name}},"image":{{json .Config.Image}},"running":{{json .State.Running}},"started":{{json .State.StartedAt}},"ports":{{json .NetworkSettings.Ports}},"project":{{json (index .Config.Labels "com.docker.compose.project")}},"directory":{{json (index .Config.Labels "com.docker.compose.project.working_dir")}},"files":{{json (index .Config.Labels "com.docker.compose.project.config_files")}}}"#;
    let mut command = crate::executables::command("docker")?;
    command
        .env_remove("DOCKER_CONTEXT")
        .env_remove("DOCKER_HOST")
        .args([
            "--host",
            &endpoint,
            "inspect",
            "--type",
            "container",
            "--format",
            FORMAT,
        ])
        .args(ids);
    let output = crate::session_environments::bounded_output(&mut command, false)
        .map_err(|_| "Não foi possível consultar os ambientes Docker. Verifique a conexão com o Docker local.".to_owned())?;
    Ok(decode(&output))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owner(id: &str) -> EnvironmentOwner {
        EnvironmentOwner {
            session_id: id.into(),
            native_id: Some(id.into()),
            agent: AgentKind::Codex,
            roots: vec![],
            working_directory: Some("/work/project".into()),
            container_commands: vec![],
        }
    }

    fn containers() -> Vec<Container> {
        decode(&serde_json::json!({"id":"a".repeat(64),"name":"/test-api","image":"example/api:local","running":true,"started":"2026-10-23T12:00:00Z", "project":"demo","directory":"/work/project","files":"/work/project/compose.yml", "ports":{"8080/tcp":[{"HostIp":"127.0.0.1","HostPort":"8080"}],"9000/udp":[{"HostIp":"127.0.0.1","HostPort":"9000"}]}}).to_string())
    }

    #[test]
    fn shell_evidence_is_explicit_and_does_not_run_output_or_quoted_examples() {
        assert_eq!(
            selectors(
                &shell_words::join(["/bin/bash", "-lc", "docker start test-api"]),
                None,
                0
            ),
            vec![Selector::Container("test-api".into())]
        );
        assert_eq!(
            selectors(
                "docker run --rm -e PASSWORD=not-stored --name test-api example/api",
                None,
                0
            ),
            vec![Selector::Container("test-api".into())]
        );
        assert_eq!(
            selectors("docker exec -it -w /app test-api sh", None, 0),
            vec![Selector::Container("test-api".into())]
        );
        assert_eq!(
            selectors("docker restart -t 10 test-api", None, 0),
            vec![Selector::Container("test-api".into())]
        );
        for text in [
            "echo docker run --name test-api example/api",
            "printf 'docker start test-api'",
            "docker inspect test-api",
            "docker ps",
            "docker run alpine echo --name test-api",
            "docker --host tcp://elsewhere run --name test-api image",
            "cat <<EOF\ndocker start test-api\nEOF",
            "echo $(docker start test-api)",
        ] {
            assert!(selectors(text, None, 0).is_empty(), "{text}");
        }
    }

    #[test]
    fn paths_are_read_as_text_the_same_way_on_every_platform() {
        assert_eq!(clean_path("/work/project/../app/./src", None).as_deref(), Some("/work/app/src"));
        assert_eq!(clean_path("sub/dir", Some("/work/project/")).as_deref(), Some("/work/project/sub/dir"));
        assert_eq!(clean_path("C:\\work\\x\\..\\y", None).as_deref(), Some("C:/work/y"));
        assert_eq!(clean_path("relative", Some("not-absolute")), None);
        assert_eq!(clean_path("relative", None), None);
        assert_eq!(clean_path("$HOME/app", Some("/work")), None);
    }

    #[test]
    fn compose_requires_a_real_lifecycle_command_not_a_directory_match() {
        let rows = containers();
        assert!(selectors("pwd", Some("/work/project"), 0).is_empty());
        assert!(selectors("docker compose ps", Some("/work/project"), 0).is_empty());
        let evidence = selectors(
            "cd /work/project && docker compose -f compose.yml up -d",
            Some("/"),
            0,
        );
        assert_eq!(evidence.len(), 1);
        assert!(matches(&rows[0], &evidence[0]));
        assert!(!matches(
            &rows[0],
            &selectors(
                "docker compose -p different up -d",
                Some("/work/project"),
                0
            )[0]
        ));
        assert!(!matches(
            &rows[0],
            &selectors("docker compose -f other.yml up", Some("/work/project"), 0)[0]
        ));
    }

    #[test]
    fn nested_calls_decode_literal_fields_and_skip_comments_expressions_and_templates() {
        observe_input(
            AgentKind::ClaudeCode,
            "serialized-container-input",
            &Value::String(r#"{"command":"docker exec test-api pwd"}"#.into()),
            None,
        );
        assert_eq!(
            claims()
                .lock()
                .unwrap()
                .get(&(AgentKind::ClaudeCode, "serialized-container-input".into()))
                .unwrap(),
            &[Selector::Container("test-api".into())]
        );
        assert_eq!(
            literal_field(
                r#"{ cmd: "docker start test-api", workdir: "/work/project" }"#,
                "cmd"
            )
            .as_deref(),
            Some("docker start test-api")
        );
        assert_eq!(
            literal_field(r#"{ /* cmd: "docker start fake" */ cmd: "pwd" }"#, "cmd").as_deref(),
            Some("pwd")
        );
        assert_eq!(
            literal_field(r#"{ ácmd: "docker start fake", cmd: "pwd" }"#, "cmd").as_deref(),
            Some("pwd")
        );
        assert!(literal_field(r#"{cmd: "docker start " + name}"#, "cmd").is_none());
        assert!(literal_field("{cmd: `docker start ${name}`} ", "cmd").is_none());
        let id = "nested-container-test";
        observe_input(AgentKind::Codex, id, &Value::String(r#"text('tools.exec_command({cmd:"docker start fake"})'); await tools.exec_command({cmd:"pwd"}); await tools.exec_command({cmd:"docker start test-api"});"#.into()), None);
        let registry = claims().lock().unwrap();
        assert_eq!(
            registry.get(&(AgentKind::Codex, id.into())).unwrap(),
            &[Selector::Container("test-api".into())]
        );
    }

    #[test]
    fn shared_containers_are_not_attributed_and_container_pids_cannot_be_stopped() {
        let first = owner("container-owner-a");
        let second = owner("container-owner-b");
        remember(
            AgentKind::Codex,
            "container-owner-a",
            "docker exec test-api pwd",
            None,
        );
        let mut inventory = ContainerInventory {
            refreshed: Some(Instant::now()),
            containers: containers(),
            error: None,
        };
        let rows = inventory
            .snapshot(&[first.clone(), second.clone()])
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].session_id, first.session_id);
        assert!(!rows[0].can_stop);
        assert_eq!(rows[0].process_id, 0);
        assert_eq!(rows[0].ports.len(), 1);
        assert_eq!(
            rows[0].ports[0].url.as_deref(),
            Some("http://127.0.0.1:8080")
        );
        remember(
            AgentKind::Codex,
            "container-owner-b",
            "docker exec test-api pwd",
            None,
        );
        assert!(inventory.snapshot(&[first, second]).unwrap().is_empty());
    }

    #[test]
    fn failed_probe_remains_unavailable_until_a_successful_refresh() {
        let actor = owner("container-unavailable");
        remember(
            AgentKind::Codex,
            "container-unavailable",
            "docker start test-api",
            None,
        );
        let mut inventory = ContainerInventory {
            refreshed: Some(Instant::now()),
            containers: containers(),
            error: Some("offline".into()),
        };
        assert!(inventory.snapshot(&[actor]).is_err());
    }
}
