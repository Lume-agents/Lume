//! Terminal presentation for `lume node`: the dino banner, readable summaries instead of raw JSON, a QR
//! code for pairing and an interactive menu. Plain JSON is still printed when the output is not a terminal
//! (pipes, scripts) or when `--json` is given.

use std::io::IsTerminal;

use console::style;
use dialoguer::{theme::ColorfulTheme, Confirm, Input, Select};
use serde_json::Value;

const BODY: &str = "M4 16h3v-2h3V9h3V6h11v2h3v8h-9v2h4v4h-5v4h-4v-5h-3v5H7v-5H5v-2H3v-4h1z";
const BELLY: &str = "M10 15h3v2h5v3h-8z";
static SHOW_QR: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Print the pairing QR code too (it is tall, so it is opt-in with `--qr`).
pub fn show_qr(enabled: bool) {
    SHOW_QR.store(enabled, std::sync::atomic::Ordering::Relaxed);
}

const DEFAULT_RELAY: &str = "https://relay.lumeagents.dev";

type Rgb = (u8, u8, u8);

/// Whether the language of the system is Portuguese.
fn portuguese() -> bool {
    if let Ok(value) = std::env::var("LUME_LANG").or_else(|_| std::env::var("LC_ALL")).or_else(|_| std::env::var("LANG")) {
        return value.to_ascii_lowercase().starts_with("pt");
    }
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        extern "system" {
            fn GetUserDefaultUILanguage() -> u16;
        }
        // SAFETY: a plain query with no arguments.
        return unsafe { GetUserDefaultUILanguage() } & 0x3ff == 0x16;
    }
    #[allow(unreachable_code)]
    false
}

fn t(english: &'static str, portuguese_text: &'static str) -> &'static str {
    if portuguese() {
        portuguese_text
    } else {
        english
    }
}

/// Turns on ANSI colors in the Windows console (a no-op elsewhere).
pub fn prepare_terminal() {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        #[link(name = "kernel32")]
        extern "system" {
            fn GetConsoleMode(handle: *mut core::ffi::c_void, mode: *mut u32) -> i32;
            fn SetConsoleMode(handle: *mut core::ffi::c_void, mode: u32) -> i32;
        }
        for handle in [std::io::stdout().as_raw_handle(), std::io::stderr().as_raw_handle()] {
            let mut mode = 0u32;
            // SAFETY: the handle comes from std and the mode pointer is valid.
            unsafe {
                if GetConsoleMode(handle as _, &mut mode) != 0 {
                    SetConsoleMode(handle as _, mode | 0x0004);
                }
            }
        }
    }
}

pub fn is_terminal() -> bool {
    std::io::stdout().is_terminal()
}

pub fn is_interactive() -> bool {
    std::io::stdin().is_terminal() && is_terminal()
}

fn colors() -> bool {
    is_terminal() && console::colors_enabled() && std::env::var_os("NO_COLOR").is_none()
}

// ── Banner ───────────────────────────────────────────────────────────────────

fn polygon(path: &str) -> Vec<(f32, f32)> {
    let (mut x, mut y) = (0f32, 0f32);
    let mut points = Vec::new();
    let mut command = ' ';
    let mut number = String::new();
    let mut values: Vec<f32> = Vec::new();
    let flush = |command: char, values: &mut Vec<f32>, x: &mut f32, y: &mut f32, points: &mut Vec<(f32, f32)>| {
        match command {
            'M' if values.len() >= 2 => {
                *x = values[0];
                *y = values[1];
            }
            'h' if !values.is_empty() => *x += values[0],
            'v' if !values.is_empty() => *y += values[0],
            'H' if !values.is_empty() => *x = values[0],
            'V' if !values.is_empty() => *y = values[0],
            _ => return,
        }
        points.push((*x, *y));
        values.clear();
    };
    for character in path.chars().chain(std::iter::once('z')) {
        if character.is_ascii_digit() || character == '-' || character == '.' {
            number.push(character);
            continue;
        }
        if !number.is_empty() {
            values.push(number.parse().unwrap_or(0.0));
            number.clear();
        }
        if character == ' ' || character == ',' {
            continue;
        }
        flush(command, &mut values, &mut x, &mut y, &mut points);
        values.clear();
        command = character;
    }
    points
}

fn inside(points: &[(f32, f32)], px: f32, py: f32) -> bool {
    let mut result = false;
    let mut previous = points.len() - 1;
    for index in 0..points.len() {
        let ((xi, yi), (xj, yj)) = (points[index], points[previous]);
        if (yi > py) != (yj > py) && px < (xj - xi) * (py - yi) / (yj - yi) + xi {
            result = !result;
        }
        previous = index;
    }
    result
}

/// The Lume dino as terminal pixels (two per character), or `None` where colors are unavailable.
fn dino_rows() -> Vec<Vec<Option<Rgb>>> {
    let (body, belly) = (polygon(BODY), polygon(BELLY));
    let mut grid = vec![vec![None; 32]; 32];
    for (y, row) in grid.iter_mut().enumerate() {
        for (x, cell) in row.iter_mut().enumerate() {
            let (cx, cy) = (x as f32 + 0.5, y as f32 + 0.5);
            if inside(&body, cx, cy) {
                *cell = Some(if inside(&belly, cx, cy) { (130, 183, 152) } else { (99, 165, 125) });
            }
        }
    }
    let mut paint = |x: usize, y: usize, w: usize, h: usize, color: Rgb| {
        for row in grid.iter_mut().skip(y).take(h) {
            for cell in row.iter_mut().skip(x).take(w) {
                *cell = Some(color);
            }
        }
    };
    paint(23, 13, 4, 2, (62, 118, 90));
    paint(21, 9, 3, 3, (234, 242, 238));
    paint(23, 9, 1, 2, (32, 50, 44));
    paint(7, 25, 5, 2, (32, 50, 44));
    paint(13, 25, 5, 2, (32, 50, 44));
    grid.into_iter().skip(6).take(22).collect::<Vec<_>>().into_iter().map(|row| row.into_iter().skip(2).take(28).collect()).collect()
}

fn dino_art() -> Vec<String> {
    let rows = dino_rows();
    rows.chunks(2)
        .map(|pair| {
            (0..pair[0].len())
                .map(|x| {
                    let (top, bottom) = (pair[0][x], pair.get(1).and_then(|row| row[x]));
                    match (top, bottom) {
                        (Some(t), Some(b)) => format!("\x1b[38;2;{};{};{};48;2;{};{};{}m▀\x1b[0m", t.0, t.1, t.2, b.0, b.1, b.2),
                        (Some(t), None) => format!("\x1b[38;2;{};{};{}m▀\x1b[0m", t.0, t.1, t.2),
                        (None, Some(b)) => format!("\x1b[38;2;{};{};{}m▄\x1b[0m", b.0, b.1, b.2),
                        (None, None) => " ".into(),
                    }
                })
                .collect()
        })
        .collect()
}

/// The dino with the name, tagline and version beside it.
pub fn banner() -> String {
    let version = env!("CARGO_PKG_VERSION");
    if !colors() {
        return format!("Lume {version} — {}\n", t("your agents, your way", "seus agentes, do seu jeito"));
    }
    let art = dino_art();
    let side = [
        String::new(),
        String::new(),
        String::new(),
        format!("{}", style("lume").bold().green()),
        format!("{}", style(t("Your agents. Your way.", "Seus agentes. Do seu jeito.")).dim()),
        format!("{}", style(format!("v{version}  ·  lumeagents.dev")).dim()),
    ];
    let mut out = String::new();
    for (index, line) in art.iter().enumerate() {
        out.push_str("  ");
        out.push_str(line);
        if let Some(text) = side.get(index) {
            out.push_str("   ");
            out.push_str(text);
        }
        out.push('\n');
    }
    out
}

// ── Readable summaries ───────────────────────────────────────────────────────

fn label(text: &str) -> String {
    format!("  {:<15}", style(text).dim())
}

fn short(value: &str) -> String {
    if value.len() > 20 {
        format!("{}…{}", &value[..8], &value[value.len() - 6..])
    } else {
        value.to_string()
    }
}

fn now_millis() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|time| time.as_millis() as i64).unwrap_or(0)
}

fn ago(millis: Option<i64>) -> String {
    let Some(millis) = millis else { return t("never", "nunca").into() };
    let seconds = ((now_millis() - millis) / 1000).max(0);
    let (value, unit_en, unit_pt) = match seconds {
        0..=59 => (seconds, "s", "s"),
        60..=3599 => (seconds / 60, "min", "min"),
        3600..=86_399 => (seconds / 3600, "h", "h"),
        _ => (seconds / 86_400, "d", "d"),
    };
    if portuguese() {
        format!("há {value} {unit_pt}")
    } else {
        format!("{value} {unit_en} ago")
    }
}

fn capability(name: &str) -> &str {
    match (name, portuguese()) {
        ("agent_inventory", true) => "inventário de agentes",
        ("agent_inventory", false) => "agent inventory",
        ("model_inventory", true) => "inventário de modelos",
        ("model_inventory", false) => "model inventory",
        ("resource_telemetry", true) => "telemetria de recursos",
        ("resource_telemetry", false) => "resource telemetry",
        (other, _) => other,
    }
}

fn gigabytes(bytes: Option<u64>) -> String {
    format!("{:.1}", bytes.unwrap_or(0) as f64 / 1_073_741_824.0)
}

fn health(value: &Value) -> String {
    let lifecycle = value["lifecycle"].as_str().unwrap_or("offline");
    let (dot, state) = match lifecycle {
        "running" => (style("●").green(), t("running", "em execução")),
        "disabled" => (style("●").red(), t("disabled", "desativado")),
        _ => (style("●").yellow(), t("not running", "parado")),
    };
    let machine = &value["machine"];
    let mut out = format!("\n  {dot} {}  {}\n\n", style(value["displayName"].as_str().unwrap_or("Lume Node")).bold(), style(format!("({state})")).dim());
    let mut row = |name: &str, text: String| out.push_str(&format!("{}{}\n", label(name), text));
    row("Node ID", value["nodeId"].as_str().unwrap_or("").to_string());
    row(t("Identity", "Identidade"), format!("{} · {}", short(value["identityFingerprint"].as_str().unwrap_or("")), value["identityAlgorithm"].as_str().unwrap_or("")));
    if let Some(address) = value["network"]["address"].as_str() {
        row(t("Network", "Rede"), format!("{address}:{}", value["network"]["port"]));
    }
    row(t("System", "Sistema"), format!(
        "{} · {} · {} CPU · {} / {} GB {}",
        machine["operatingSystem"].as_str().unwrap_or("?"), machine["architecture"].as_str().unwrap_or("?"), machine["logicalCpuCount"],
        gigabytes(machine["availableMemoryBytes"].as_u64()), gigabytes(machine["totalMemoryBytes"].as_u64()), t("free", "livres")
    ));
    let capabilities = value["capabilities"].as_array().map(|list| list.iter().filter_map(|item| item.as_str()).map(capability).collect::<Vec<_>>().join(", ")).unwrap_or_default();
    row(t("Capabilities", "Capacidades"), capabilities);
    row(t("Last signal", "Último sinal"), ago(value["heartbeatAt"].as_i64()));
    out
}

fn list(items: &Value, empty: &str, line: impl Fn(&Value) -> String) -> String {
    match items.as_array() {
        Some(items) if !items.is_empty() => format!("\n{}\n", items.iter().map(line).collect::<Vec<_>>().join("\n")),
        _ => format!("\n  {}\n", style(empty).dim()),
    }
}

fn pairing(value: &Value) -> String {
    let uri = value["relayPairingUri"].as_str().or_else(|| value["qrPayload"].as_str()).unwrap_or_default();
    let mut out = format!("\n  {}\n\n", style(t("Paste this link on the other device.", "Cole este link no outro dispositivo.")).bold());
    if let (true, Ok(code)) = (SHOW_QR.load(std::sync::atomic::Ordering::Relaxed), qrcode::QrCode::new(uri.as_bytes())) {
        let art = code
            .render::<qrcode::render::unicode::Dense1x2>()
            .dark_color(qrcode::render::unicode::Dense1x2::Light)
            .light_color(qrcode::render::unicode::Dense1x2::Dark)
            .quiet_zone(true)
            .build();
        for line in art.lines() {
            out.push_str(&format!("  {line}\n"));
        }
    }
    let command = if value["relayPairingUri"].is_string() { "relay-pair" } else { "connect" };
    out.push_str(&format!("\n{}{}\n", label(t("Link", "Link")), style(uri).cyan()));
    out.push_str(&format!("{}{}\n", label(t("Valid for", "Vale por")), t("5 minutes, single use", "5 minutos, uso único")));
    out.push_str(&format!("{}lume node {command} '<{}>'\n", label(t("On the other PC", "No outro PC")), t("link", "link")));
    out
}

/// A readable version of a command's JSON result, or `None` to keep the JSON.
pub fn pretty(command: &str, value: &Value) -> Option<String> {
    Some(match command {
        "status" | "health" | "relay-health" | "enable" | "disable" => health(value),
        "pair" => pairing(value),
        "clients" => list(value, t("No device is paired with this Node yet.", "Nenhum dispositivo está pareado com este Node ainda."), |client| {
            format!(
                "  {} {}  {}\n{}{}\n{}{}",
                style("●").green(), style(client["displayName"].as_str().unwrap_or("?")).bold(),
                style(format!("({})", short(client["identity"]["fingerprint"].as_str().unwrap_or("")))).dim(),
                label(t("Access", "Acesso")), client["scopes"].as_array().map(|scopes| scopes.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")).unwrap_or_default(),
                label(t("Last seen", "Visto")), ago(client["lastSeenAt"].as_i64()),
            )
        }),
        "remotes" | "relay-remotes" => list(value, t("No computer is paired yet.", "Nenhum computador pareado ainda."), |remote| {
            let via = remote["relayUrl"].as_str().map(str::to_string).unwrap_or_else(|| format!("{}:{}", remote["address"].as_str().unwrap_or("?"), remote["port"]));
            format!(
                "  {} {}\n{}{}\n{}{}",
                style("●").cyan(), style(remote["nodeId"].as_str().unwrap_or("?")).bold(),
                label(t("Via", "Por")), via,
                label(t("Paired", "Pareado")), ago(remote["pairedAt"].as_i64()),
            )
        }),
        "relay" => {
            let url = value["relayUrl"].as_str();
            format!(
                "\n  {} {}\n  {}\n",
                style("✔").green(),
                match url {
                    Some(url) => format!("{} {}", t("Relay set to", "Relay definido:"), style(url).cyan()),
                    None => t("Relay turned off", "Relay desligado").to_string(),
                },
                style(t("Restart `lume node run` for it to take effect.", "Reinicie o `lume node run` para valer.")).dim()
            )
        }
        _ => return None,
    })
}

// ── Interactive menu ─────────────────────────────────────────────────────────

/// Runs the menu; `run` executes the chosen commands.
/// Runs a `lume node` command and returns the JSON text it would print.
pub type CommandRunner<'a> = &'a dyn Fn(&[String]) -> Result<Option<String>, String>;

pub fn menu(state_arguments: &[String], run: CommandRunner, run_service: &dyn Fn() -> Result<(), String>) -> Result<(), String> {
    prepare_terminal();
    println!("{}", banner());
    let theme = ColorfulTheme::default();
    let command = |name: &str, extra: &[&str]| -> Vec<String> {
        let mut arguments = vec![name.to_string()];
        arguments.extend(extra.iter().map(|value| value.to_string()));
        arguments.extend_from_slice(state_arguments);
        arguments
    };
    let show = |name: &str, result: Result<Option<String>, String>| match result {
        Ok(Some(text)) => match serde_json::from_str::<Value>(&text).ok().and_then(|value| pretty(name, &value)) {
            Some(rendered) => println!("{rendered}"),
            None => println!("{text}"),
        },
        Ok(None) => {}
        Err(error) => eprintln!("\n  {} {error}\n", style("✘").red()),
    };
    let items = [
        t("Node status", "Estado do Node"),
        t("Turn the Node on", "Ativar o Node"),
        t("Connect to the Relay (access from other networks)", "Conectar ao Relay (acesso de outras redes)"),
        t("Start the Node now", "Iniciar o Node agora"),
        t("Pair another device with this Node", "Parear outro dispositivo com este Node"),
        t("Pair this computer with another Node", "Parear este computador com outro Node"),
        t("Devices authorized on this Node", "Dispositivos autorizados neste Node"),
        t("Paired computers", "Computadores pareados"),
        t("Check a paired computer", "Verificar um computador pareado"),
        t("Quit", "Sair"),
    ];
    loop {
        let choice = Select::with_theme(&theme).with_prompt(t("What do you want to do?", "O que você quer fazer?")).items(&items).default(0).interact_opt().map_err(|error| error.to_string())?;
        let Some(choice) = choice else { return Ok(()) };
        println!();
        match choice {
            0 => show("status", run(&command("status", &[]))),
            1 => show("enable", run(&command("enable", &[]))),
            2 => {
                let url: String = Input::with_theme(&theme).with_prompt(t("Relay address", "Endereço do Relay")).default(DEFAULT_RELAY.into()).interact_text().map_err(|error| error.to_string())?;
                show("relay", run(&command("relay", &[&url])));
            }
            3 => {
                println!("  {}\n", style(t("The Node is running. Press Ctrl+C to stop it.", "O Node está rodando. Pressione Ctrl+C para parar.")).dim());
                return run_service();
            }
            4 => show("pair", run(&command("pair", &[]))),
            5 => {
                let uri: String = Input::with_theme(&theme).with_prompt(t("Paste the pairing link", "Cole o link de pareamento")).interact_text().map_err(|error| error.to_string())?;
                let name = if uri.trim().starts_with("lume://pair-relay") { "relay-pair" } else { "connect" };
                show(name, run(&command(name, &[uri.trim()])));
            }
            6 => show("clients", run(&command("clients", &[]))),
            7 => show("relay-remotes", run(&command("relay-remotes", &[]))),
            8 => {
                let remotes = run(&command("relay-remotes", &[])).ok().flatten().and_then(|text| serde_json::from_str::<Value>(&text).ok()).and_then(|value| value.as_array().cloned()).unwrap_or_default();
                if remotes.is_empty() {
                    println!("  {}\n", style(t("No computer is paired yet.", "Nenhum computador pareado ainda.")).dim());
                    continue;
                }
                let names = remotes.iter().map(|remote| remote["nodeId"].as_str().unwrap_or("?").to_string()).collect::<Vec<_>>();
                let picked = Select::with_theme(&theme).with_prompt(t("Which one?", "Qual?")).items(&names).default(0).interact().map_err(|error| error.to_string())?;
                show("relay-health", run(&command("relay-health", &[&names[picked]])));
            }
            _ => return Ok(()),
        }
        if !Confirm::with_theme(&theme).with_prompt(t("Back to the menu?", "Voltar ao menu?")).default(true).interact().map_err(|error| error.to_string())? {
            return Ok(());
        }
        println!();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_dino_is_drawn_from_the_mascot_outline() {
        let rows = dino_rows();
        assert_eq!((rows.len(), rows[0].len()), (22, 28));
        let filled = rows.iter().flatten().filter(|cell| cell.is_some()).count();
        assert!(filled > 180 && filled < 420, "{filled} pixels");
        assert_eq!(dino_art().len(), 11, "two pixel rows per text line");
    }

    #[test]
    fn long_identifiers_are_shortened_and_times_are_relative() {
        assert_eq!(short("3d166f73e17d5e686709a1f88de7aa764ccf889c9f354b0932b2c948c84e5196"), "3d166f73…4e5196");
        assert_eq!(short("node-1"), "node-1");
        assert!(ago(None).len() >= 4);
        assert!(ago(Some(now_millis() - 5 * 60_000)).contains("5 min"));
    }

    #[test]
    fn summaries_show_the_useful_fields_and_unknown_commands_keep_the_json() {
        let node = json!({ "lifecycle": "running", "displayName": "pop-os", "nodeId": "node-1", "identityAlgorithm": "Ed25519", "identityFingerprint": "a".repeat(64),
            "capabilities": ["agent_inventory"], "heartbeatAt": now_millis(), "machine": { "operatingSystem": "Linux", "architecture": "x86_64", "logicalCpuCount": 8, "availableMemoryBytes": 1073741824u64, "totalMemoryBytes": 2147483648u64 } });
        let text = console::strip_ansi_codes(&pretty("status", &node).unwrap()).to_string();
        assert!(text.contains("pop-os") && text.contains("node-1") && text.contains("8 CPU") && text.contains("1.0 / 2.0 GB"));
        assert!(pretty("inventory", &node).is_none());
        let paired = json!({ "relayPairingUri": "lume://pair-relay?v=1&relay=https://r&node=n&offer=o&secret=s&fingerprint=f" });
        assert!(console::strip_ansi_codes(&pretty("pair", &paired).unwrap()).contains("lume://pair-relay?v=1"));
    }
}
