//! Multi-machine agent hosting: body identity and runner role.
//!
//! One agent key may be hosted by several Buzz installs (two desktops, a
//! laptop and a server). Each install is a *body* of that identity. The owner
//! assigns an agent to a machine (`ManagedAgentRecord::assigned_machine`,
//! shared through the kind:30177 projection); each install compares that to
//! its own machine name (`GlobalAgentConfig::machine_name`, local only) and
//! spawns the harness as `active` or `standby`. The harness turn-claim lease
//! (`crates/buzz-acp/src/turn_claim.rs`) then guarantees one answer per
//! mention and fails over to a standby body when the active one is down.

use super::{GlobalAgentConfig, ManagedAgentRecord};

/// Env var carrying this body's identifier to the harness.
pub const BODY_ID_ENV: &str = "BUZZ_ACP_BODY_ID";
/// Env var carrying the body role (`active` | `standby`) to the harness.
pub const RUNNER_MODE_ENV: &str = "BUZZ_ACP_RUNNER_MODE";
/// Upper bound on a machine name; long enough for any hostname, short enough
/// to keep claim events and logs readable.
pub const MAX_MACHINE_NAME_LEN: usize = 64;
/// Fallback body id when neither a configured name nor a hostname is available.
const FALLBACK_BODY_ID: &str = "this-machine";

/// Trim a user-entered machine name; empty input means "unset".
pub fn normalize_machine_name(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Validate a normalized machine name: bounded length, no control characters.
pub fn validate_machine_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("machine name must not be empty".to_string());
    }
    if name.chars().count() > MAX_MACHINE_NAME_LEN {
        return Err(format!(
            "machine name exceeds {MAX_MACHINE_NAME_LEN} characters"
        ));
    }
    if name.chars().any(char::is_control) {
        return Err("machine name must not contain control characters".to_string());
    }
    Ok(())
}

/// Short hostname of this machine (`Mac-mini-2.local` -> `Mac-mini-2`).
fn short_hostname() -> Option<String> {
    let output = std::process::Command::new("hostname").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let host = text.trim();
    if host.is_empty() {
        return None;
    }
    Some(host.split('.').next().unwrap_or(host).to_string())
}

/// This install's body id: the configured machine name, else the hostname.
pub fn local_body_id(global: &GlobalAgentConfig) -> String {
    normalize_machine_name(global.machine_name.as_deref())
        .or_else(short_hostname)
        .unwrap_or_else(|| FALLBACK_BODY_ID.to_string())
}

/// Role this install plays for `record`: unassigned agents run everywhere
/// (legacy behavior); assigned agents run actively only on the named machine
/// and in standby elsewhere. Names compare case-insensitively.
pub fn runner_mode_for(record: &ManagedAgentRecord, body_id: &str) -> &'static str {
    match normalize_machine_name(record.assigned_machine.as_deref()) {
        None => "active",
        Some(assigned) if assigned.eq_ignore_ascii_case(body_id.trim()) => "active",
        Some(_) => "standby",
    }
}

/// Apply body id and runner mode to a harness spawn command.
pub fn apply_runner_body_env(
    command: &mut std::process::Command,
    record: &ManagedAgentRecord,
    global: &GlobalAgentConfig,
) {
    let body_id = local_body_id(global);
    let mode = runner_mode_for(record, &body_id);
    command.env(BODY_ID_ENV, &body_id);
    command.env(RUNNER_MODE_ENV, mode);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(assigned: Option<&str>) -> ManagedAgentRecord {
        let mut record: ManagedAgentRecord = serde_json::from_value(serde_json::json!({
            "pubkey": "ab".repeat(32),
            "name": "Hive",
            "relay_url": "wss://relay.example",
            "acp_command": "buzz-acp",
            "agent_command": "claude",
            "agent_args": [],
            "mcp_command": "",
            "turn_timeout_seconds": 320,
            "system_prompt": null,
            "persona_team_dir": null,
            "persona_name_in_team": null,
            "created_at": "2026-09-30T00:00:00Z",
            "updated_at": "2026-09-30T00:00:00Z",
            "last_started_at": null,
            "last_stopped_at": null,
            "last_exit_code": null,
            "last_error": null
        }))
        .expect("minimal record deserializes");
        record.assigned_machine = assigned.map(str::to_string);
        record
    }

    #[test]
    fn normalize_trims_and_drops_empty() {
        assert_eq!(
            normalize_machine_name(Some("  mini-2 ")),
            Some("mini-2".into())
        );
        assert_eq!(normalize_machine_name(Some("   ")), None);
        assert_eq!(normalize_machine_name(None), None);
    }

    #[test]
    fn validate_rejects_long_and_control_names() {
        assert!(validate_machine_name("Mac-mini-2").is_ok());
        assert!(validate_machine_name("").is_err());
        assert!(validate_machine_name(&"x".repeat(MAX_MACHINE_NAME_LEN + 1)).is_err());
        assert!(validate_machine_name("bad\nname").is_err());
    }

    #[test]
    fn unassigned_runs_active_everywhere() {
        assert_eq!(runner_mode_for(&record(None), "any"), "active");
        assert_eq!(runner_mode_for(&record(Some("  ")), "any"), "active");
    }

    #[test]
    fn assigned_runs_active_only_on_named_machine_case_insensitive() {
        let r = record(Some("Mac-mini-2"));
        assert_eq!(runner_mode_for(&r, "mac-mini-2"), "active");
        assert_eq!(runner_mode_for(&r, "Andrews-Mini"), "standby");
    }

    #[test]
    fn local_body_id_prefers_configured_name() {
        let global = GlobalAgentConfig {
            machine_name: Some(" studio ".into()),
            ..Default::default()
        };
        assert_eq!(local_body_id(&global), "studio");
        let unset = GlobalAgentConfig::default();
        let id = local_body_id(&unset);
        assert!(!id.is_empty());
        assert!(!id.contains('.'));
    }

    #[test]
    fn env_carries_body_and_mode() {
        let global = GlobalAgentConfig {
            machine_name: Some("mini-2".into()),
            ..Default::default()
        };
        let mut cmd = std::process::Command::new("true");
        apply_runner_body_env(&mut cmd, &record(Some("other")), &global);
        let envs: Vec<(String, String)> = cmd
            .get_envs()
            .filter_map(|(k, v)| {
                Some((
                    k.to_string_lossy().into_owned(),
                    v?.to_string_lossy().into_owned(),
                ))
            })
            .collect();
        assert!(envs.contains(&(BODY_ID_ENV.to_string(), "mini-2".to_string())));
        assert!(envs.contains(&(RUNNER_MODE_ENV.to_string(), "standby".to_string())));
    }
}
