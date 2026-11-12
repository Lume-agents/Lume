//! What other devices did on this computer through the Relay: pairing, authenticating, prompts, approvals and stops.
//!
//! One JSON object per line in `remote-audit.jsonl` in the Node's data directory, readable only by its owner, with
//! one previous file kept when it grows past 1 MiB. A record says who asked for what and how it ended; it never holds
//! the text of a prompt (only its length), an authenticator code or a session grant. `lume node audit` prints it.

use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

const FILE: &str = "remote-audit.jsonl";
const PREVIOUS_FILE: &str = "remote-audit.1.jsonl";
const MAX_BYTES: u64 = 1024 * 1024;
const MAX_REASON: usize = 160;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditRecord {
    pub at: i64,
    pub device_id: String,
    pub fingerprint: String,
    pub device_name: String,
    pub cmd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    pub route: String,
    /// `accepted`, `repeated` (answered from memory, not run again) or `refused:<reason>`.
    pub outcome: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_length: Option<usize>,
}

/// The outcome of a command from the answer the Node sends back.
pub fn outcome_of(answer: &Value) -> String {
    if answer["ok"] == true {
        return if answer["result"]["repeated"] == true { "repeated" } else { "accepted" }.into();
    }
    refused(answer["error"].as_str().unwrap_or("unknown"))
}

/// `refused:<reason>`, on one line and bounded, whatever the error said.
pub fn refused(reason: &str) -> String {
    let reason: String = reason.chars().map(|c| if c.is_control() { ' ' } else { c }).take(MAX_REASON).collect();
    format!("refused:{reason}")
}

fn open_private(path: &Path) -> std::io::Result<fs::File> {
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

/// Appends one record. Auditing never stops the command it describes, so a failure only returns the error.
pub fn record(directory: &Path, record: &AuditRecord) -> Result<(), String> {
    let path = directory.join(FILE);
    if fs::metadata(&path).is_ok_and(|metadata| metadata.len() >= MAX_BYTES) {
        fs::rename(&path, directory.join(PREVIOUS_FILE)).map_err(|error| error.to_string())?;
    }
    let mut line = serde_json::to_vec(record).map_err(|error| error.to_string())?;
    line.push(b'\n');
    let mut file = open_private(&path).map_err(|error| error.to_string())?;
    file.write_all(&line).map_err(|error| error.to_string())
}

/// The most recent `limit` records, oldest first, across the current and the previous file.
pub fn recent(directory: &Path, limit: usize) -> Vec<AuditRecord> {
    let mut records: Vec<AuditRecord> = [PREVIOUS_FILE, FILE]
        .iter()
        .filter_map(|name| fs::read_to_string(directory.join(name)).ok())
        .flat_map(|text| text.lines().filter_map(|line| serde_json::from_str(line).ok()).collect::<Vec<_>>())
        .collect();
    let skip = records.len().saturating_sub(limit);
    records.drain(..skip);
    records
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn directory(name: &str) -> std::path::PathBuf {
        let directory = std::env::temp_dir().join(format!("lume-audit-{name}-{}-{}", std::process::id(), crate::state::now_millis()));
        fs::create_dir_all(&directory).unwrap();
        directory
    }

    fn prompt(at: i64) -> AuditRecord {
        AuditRecord {
            at, device_id: "device-1".into(), fingerprint: "ab".repeat(32), device_name: "Laptop".into(), cmd: "prompt".into(),
            session_id: Some("s1".into()), request_id: Some("request-0001".into()), route: "relay".into(), outcome: "accepted".into(), prompt_length: Some(17),
        }
    }

    #[test]
    fn a_record_is_one_line_with_the_prompt_length_and_never_its_text() {
        let directory = directory("format");
        record(&directory, &prompt(1)).unwrap();
        let text = fs::read_to_string(directory.join(FILE)).unwrap();
        assert_eq!(text.lines().count(), 1);
        let line: Value = serde_json::from_str(text.trim()).unwrap();
        assert_eq!(line["promptLength"], 17);
        assert_eq!((line["route"].as_str(), line["outcome"].as_str(), line["deviceName"].as_str()), (Some("relay"), Some("accepted"), Some("Laptop")));
        assert!(line.get("prompt").is_none() && line.get("code").is_none());
        let fields: Vec<&String> = line.as_object().unwrap().keys().collect();
        assert_eq!(fields.len(), 10, "{fields:?}");
        assert_eq!(recent(&directory, 10), [prompt(1)]);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(directory.join(FILE)).unwrap().permissions().mode() & 0o777, 0o600);
        }
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn outcomes_say_accepted_repeated_or_why_it_was_refused() {
        assert_eq!(outcome_of(&json!({ "ok": true, "result": { "accepted": true } })), "accepted");
        assert_eq!(outcome_of(&json!({ "ok": true, "result": { "accepted": true, "repeated": true } })), "repeated");
        assert_eq!(outcome_of(&json!({ "ok": false, "error": "too\nmany" })), "refused:too many");
        assert_eq!(refused(&"x".repeat(1000)).len(), "refused:".len() + MAX_REASON);
    }

    #[test]
    fn the_file_rotates_and_keeps_one_previous_file() {
        let directory = directory("rotate");
        fs::write(directory.join(FILE), vec![b'\n'; MAX_BYTES as usize]).unwrap();
        record(&directory, &prompt(2)).unwrap();
        assert!(fs::metadata(directory.join(PREVIOUS_FILE)).unwrap().len() >= MAX_BYTES);
        assert_eq!(fs::read_to_string(directory.join(FILE)).unwrap().lines().count(), 1);
        fs::write(directory.join(FILE), vec![b'\n'; MAX_BYTES as usize]).unwrap();
        record(&directory, &prompt(3)).unwrap();
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 2, "only one previous file is kept");
        assert_eq!(recent(&directory, 10), [prompt(3)]);
        for at in 4..8 { record(&directory, &prompt(at)).unwrap(); }
        assert_eq!(recent(&directory, 2).iter().map(|record| record.at).collect::<Vec<_>>(), [6, 7]);
        let _ = fs::remove_dir_all(directory);
    }
}
