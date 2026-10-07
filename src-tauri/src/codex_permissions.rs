//! The permission modes Lume offers for a Codex conversation, and what each one
//! asks of the App Server on a turn: when to ask, who reviews, and the sandbox.

use serde_json::{json, Value};

use crate::domain::{AccessMode, PermissionProfile, PermissionSettings};

/// Normal asks you, "approve for me" lets Codex's reviewer decide, and full
/// access runs without a sandbox or questions.
pub const MODES: [&str; 3] = ["default", "auto_review", "full_access"];

pub fn is_mode(mode: &str) -> bool {
    MODES.contains(&mode)
}

/// The offered modes, plus the one in effect when Lume does not offer it (read only).
pub fn settings(mode: &str) -> PermissionSettings {
    let mut modes = MODES
        .iter()
        .map(|mode| mode.to_string())
        .collect::<Vec<_>>();
    if !modes.iter().any(|candidate| candidate == mode) {
        modes.push(mode.to_string());
    }
    PermissionSettings {
        mode: mode.to_string(),
        modes,
    }
}

/// The mode a session's reported profile corresponds to.
pub fn mode_of(profile: &PermissionProfile) -> &'static str {
    match profile.mode {
        AccessMode::FullAccess => "full_access",
        AccessMode::ReadOnly | AccessMode::Plan => "read_only",
        AccessMode::WorkspaceWrite | AccessMode::Custom => {
            if profile.automatically_approves() {
                "auto_review"
            } else {
                "default"
            }
        }
    }
}

/// `approvalPolicy`, `approvalsReviewer` and `sandboxPolicy` for a `turn/start`.
/// Every mode states all three, so leaving full access restores the sandbox.
pub fn turn_params(mode: &str) -> Option<[(&'static str, Value); 3]> {
    let workspace_write = json!({
        "type": "workspaceWrite",
        "writableRoots": [],
        "networkAccess": false,
        "excludeTmpdirEnvVar": false,
        "excludeSlashTmp": false,
    });
    let (policy, reviewer, sandbox) = match mode {
        "default" => ("on-request", "user", workspace_write),
        "auto_review" => ("on-request", "auto_review", workspace_write),
        "full_access" => ("never", "user", json!({ "type": "dangerFullAccess" })),
        _ => return None,
    };
    Some([
        ("approvalPolicy", json!(policy)),
        ("approvalsReviewer", json!(reviewer)),
        ("sandboxPolicy", sandbox),
    ])
}

/// What the session's permission badge shows once the mode is picked.
pub fn scope(mode: &str) -> Option<PermissionProfile> {
    let (access, label, policy, reviewer) = match mode {
        "default" => (
            AccessMode::WorkspaceWrite,
            "Acesso ao projeto",
            "on-request",
            None,
        ),
        "auto_review" => (
            AccessMode::WorkspaceWrite,
            "Acesso ao projeto",
            "on-request",
            Some("auto_review"),
        ),
        "full_access" => (AccessMode::FullAccess, "Acesso total", "never", None),
        _ => return None,
    };
    Some(PermissionProfile {
        mode: access,
        label: label.into(),
        approval_policy: policy.into(),
        approvals_reviewer: Some(reviewer.unwrap_or("user").into()),
        can_respond_from_lume: true,
        available_actions: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(mode: AccessMode, reviewer: Option<&str>) -> PermissionProfile {
        PermissionProfile {
            mode,
            label: String::new(),
            approval_policy: "on-request".into(),
            approvals_reviewer: reviewer.map(str::to_string),
            can_respond_from_lume: true,
            available_actions: Vec::new(),
        }
    }

    #[test]
    fn a_session_profile_maps_back_to_a_mode() {
        assert_eq!(
            mode_of(&profile(AccessMode::WorkspaceWrite, Some("user"))),
            "default"
        );
        assert_eq!(mode_of(&profile(AccessMode::Custom, None)), "default");
        assert_eq!(
            mode_of(&profile(AccessMode::WorkspaceWrite, Some("auto_review"))),
            "auto_review"
        );
        assert_eq!(
            mode_of(&profile(AccessMode::FullAccess, Some("user"))),
            "full_access"
        );
        assert_eq!(mode_of(&profile(AccessMode::ReadOnly, None)), "read_only");
    }

    #[test]
    fn every_mode_states_policy_reviewer_and_sandbox() {
        let get = |mode: &str, key: &str| {
            turn_params(mode)
                .unwrap()
                .into_iter()
                .find(|(name, _)| *name == key)
                .unwrap()
                .1
        };
        assert_eq!(get("default", "approvalsReviewer"), json!("user"));
        assert_eq!(
            get("auto_review", "approvalsReviewer"),
            json!("auto_review")
        );
        assert_eq!(get("auto_review", "approvalPolicy"), json!("on-request"));
        assert_eq!(get("full_access", "approvalPolicy"), json!("never"));
        assert_eq!(
            get("full_access", "sandboxPolicy"),
            json!({ "type": "dangerFullAccess" })
        );
        // Returning to normal restores the project sandbox.
        assert_eq!(
            get("default", "sandboxPolicy")["type"],
            json!("workspaceWrite")
        );
        assert!(turn_params("read_only").is_none() && !is_mode("read_only"));
    }

    #[test]
    fn the_scope_shown_matches_the_mode_picked() {
        let auto = scope("auto_review").unwrap();
        assert!(auto.automatically_approves() && auto.mode == AccessMode::WorkspaceWrite);
        assert!(!scope("default").unwrap().automatically_approves());
        assert_eq!(scope("full_access").unwrap().mode, AccessMode::FullAccess);
        assert_eq!(
            settings("read_only").modes.last().map(String::as_str),
            Some("read_only")
        );
    }
}
