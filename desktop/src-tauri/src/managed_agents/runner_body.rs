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
/// When `true`, the harness may put this body's name on member-visible
/// events and export it to the agent as `BUZZ_BODY_ID`. Set only for an
/// explicit Agent hosting name; the hostname fallback publishes nothing.
pub const PUBLISH_BODY_ENV: &str = "BUZZ_ACP_PUBLISH_BODY";
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

/// True when Agent hosting has an explicit machine name safe to publish.
///
/// The hostname fallback still claims and logs under `local_body_id`, and it
/// does not publish. A name equal to the hostname is explicit: the body id
/// string is unchanged, but the publish flag flips.
pub fn publishes_explicit_body(global: &GlobalAgentConfig) -> bool {
    normalize_machine_name(global.machine_name.as_deref())
        .is_some_and(|name| validate_machine_name(&name).is_ok())
}

/// Would the runner-body spawn env differ for `record`?
///
/// `BUZZ_ACP_BODY_ID`, `BUZZ_ACP_RUNNER_MODE`, and `BUZZ_ACP_PUBLISH_BODY` are
/// baked in at spawn and are not part of `EffectiveAgentEnv`. A save that
/// sets or clears a name equal to the hostname leaves the body id string
/// unchanged (`local_body_id` falls back to that hostname) and still flips
/// the publish flag, so both directions must restart running agents.
pub fn runner_body_changed(
    record: &ManagedAgentRecord,
    old_body: &str,
    new_body: &str,
    old_publish: bool,
    new_publish: bool,
) -> bool {
    old_body != new_body
        || runner_mode_for(record, old_body) != runner_mode_for(record, new_body)
        || old_publish != new_publish
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
    // Always set the flag so a parent environment cannot leave a stale
    // `true` on a harness that should be on the hostname fallback.
    command.env(
        PUBLISH_BODY_ENV,
        if publishes_explicit_body(global) {
            "true"
        } else {
            "false"
        },
    );
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
    fn machine_rename_changes_runner_body_env() {
        // Same name, same role: nothing to restart.
        assert!(!runner_body_changed(
            &record(None),
            "mini-2",
            "mini-2",
            false,
            false
        ));
        // Renaming the body changes the claim identity even for unassigned agents.
        assert!(runner_body_changed(
            &record(None),
            "mini-2",
            "studio",
            false,
            false
        ));
        // Renaming onto/off the assigned machine flips active <-> standby.
        assert!(runner_body_changed(
            &record(Some("studio")),
            "mini-2",
            "studio",
            true,
            true
        ));
        assert!(runner_body_changed(
            &record(Some("mini-2")),
            "mini-2",
            "studio",
            true,
            true
        ));
        // Case-only rename keeps the role but still changes the body id.
        assert!(runner_body_changed(
            &record(Some("mini-2")),
            "mini-2",
            "Mini-2",
            true,
            true
        ));
    }

    #[test]
    fn publish_flag_restarts_when_the_body_id_string_is_unchanged() {
        let record = record(None);
        // Hostname fallback → explicit name equal to that hostname.
        assert!(runner_body_changed(
            &record, "mini-2", "mini-2", false, true
        ));
        // Explicit name cleared; the hostname fallback keeps the same body id.
        assert!(runner_body_changed(
            &record, "mini-2", "mini-2", true, false
        ));
        assert!(!runner_body_changed(
            &record, "mini-2", "mini-2", true, true
        ));
        assert!(!runner_body_changed(
            &record, "mini-2", "mini-2", false, false
        ));
    }

    #[test]
    fn publishes_only_an_explicit_valid_name() {
        let named = GlobalAgentConfig {
            machine_name: Some(" mini-2 ".into()),
            ..Default::default()
        };
        assert!(publishes_explicit_body(&named));
        assert!(!publishes_explicit_body(&GlobalAgentConfig::default()));
        let blank = GlobalAgentConfig {
            machine_name: Some("   ".into()),
            ..Default::default()
        };
        assert!(!publishes_explicit_body(&blank));
        let controls = GlobalAgentConfig {
            machine_name: Some("bad\nname".into()),
            ..Default::default()
        };
        assert!(!publishes_explicit_body(&controls));
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
        assert!(envs.contains(&(PUBLISH_BODY_ENV.to_string(), "true".to_string())));

        let fallback = GlobalAgentConfig::default();
        let mut quiet = std::process::Command::new("true");
        apply_runner_body_env(&mut quiet, &record(None), &fallback);
        let publish = quiet
            .get_envs()
            .find(|(k, _)| k == &std::ffi::OsStr::new(PUBLISH_BODY_ENV))
            .and_then(|(_, v)| v)
            .map(|v| v.to_string_lossy().into_owned());
        assert_eq!(publish.as_deref(), Some("false"));
    }
}
