//! Tells the person at this computer when another device asks its Node to pair, whichever Lume screen is open.
//!
//! The approval itself stays in Settings → Remote computers (or `lume node approve`); this only raises the
//! hand: an event every window turns into a banner, plus a desktop notification when those are enabled.

use std::{collections::HashSet, thread, time::Duration};

use tauri::{AppHandle, Emitter};
use tauri_plugin_notification::NotificationExt;

use crate::{
    node_identity::NodeIdentity,
    node_pairing::{self, PendingPairRequest},
    node_service,
    state::{now_millis, AppState},
};

const POLL: Duration = Duration::from_secs(3);
pub const EVENT: &str = "lume://node-pair-request";

/// The requests not announced yet; forgets the ones that are gone so a device asking again is announced again.
fn unannounced(seen: &mut HashSet<String>, pending: Vec<PendingPairRequest>) -> Vec<PendingPairRequest> {
    let current: HashSet<String> = pending.iter().map(|request| request.request_id.clone()).collect();
    seen.retain(|id| current.contains(id));
    pending.into_iter().filter(|request| seen.insert(request.request_id.clone())).collect()
}

fn notification_text(request: &PendingPairRequest, english: bool) -> (String, String) {
    let code = if request.code.len() == 6 { format!("{} {}", &request.code[..3], &request.code[3..]) } else { request.code.clone() };
    if english {
        ("Lume · Pairing request".into(), format!("{} wants to see this computer's agents. Code {code}. Open Settings → Remote computers to answer.", request.display_name))
    } else {
        ("Lume · Pedido de pareamento".into(), format!("{} quer ver os agentes deste computador. Código {code}. Abra Configurações → Computadores remotos para responder.", request.display_name))
    }
}

pub fn start(state: AppState, app: AppHandle) {
    thread::spawn(move || {
        let mut seen = HashSet::new();
        loop {
            thread::sleep(POLL);
            let Ok(directory) = node_service::default_state_directory() else { continue };
            if !directory.join("pairing.json").is_file() {
                continue;
            }
            let Ok(identity) = NodeIdentity::load_or_create(&directory) else { continue };
            let Ok(pending) = node_pairing::pending_requests(&directory, &identity.public().fingerprint, now_millis()) else { continue };
            for request in unannounced(&mut seen, pending) {
                let _ = app.emit(EVENT, &request);
                let preferences = state.preferences().ok();
                if preferences.as_ref().is_some_and(|preferences| preferences.popup_notifications_enabled) {
                    let english = preferences.is_some_and(|preferences| preferences.language != "pt-BR");
                    let (title, body) = notification_text(&request, english);
                    let _ = app.notification().builder().title(title).body(body).show();
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(id: &str) -> PendingPairRequest {
        PendingPairRequest { request_id: id.into(), display_name: "Phone".into(), client_fingerprint: "f".into(), code: "120045".into(), created_at: 1, expires_at: 2 }
    }

    #[test]
    fn each_request_is_announced_once_and_again_if_it_returns() {
        let mut seen = HashSet::new();
        assert_eq!(unannounced(&mut seen, vec![request("a")]).len(), 1);
        assert!(unannounced(&mut seen, vec![request("a")]).is_empty());
        let both = unannounced(&mut seen, vec![request("a"), request("b")]);
        assert_eq!(both.iter().map(|item| item.request_id.as_str()).collect::<Vec<_>>(), ["b"]);
        assert!(unannounced(&mut seen, vec![]).is_empty());
        assert_eq!(unannounced(&mut seen, vec![request("a")]).len(), 1, "a request that came back is a new request");
    }

    #[test]
    fn the_notification_shows_the_code_in_both_languages() {
        let (_, english) = notification_text(&request("a"), true);
        let (title, portuguese) = notification_text(&request("a"), false);
        assert!(english.contains("120 045") && portuguese.contains("120 045"));
        assert!(title.contains("Pedido"));
    }
}
