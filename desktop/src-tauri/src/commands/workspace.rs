use nostr::Keys;
use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Mutex,
};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::app_state::AppState;
use crate::managed_agents::{
    effective_repos_dir, ensure_repos_symlink, nest_dir, restore_managed_agents_on_launch,
    try_regenerate_nest, write_persisted_repos_dir,
};
use crate::relay;

/// Adopt the pre-scoping global retention database's pending rows into `scope`.
///
/// Best-effort: a failure is logged and the boot proceeds. The migration's own
/// crash-safety guards make the next launch retry safely, and blocking the
/// workspace apply on it would be worse than a delayed publish.
fn migrate_legacy_retention_into(
    app: &AppHandle,
    scope: &crate::managed_agents::retention::RetentionScope,
) {
    let Ok(base_dir) = crate::managed_agents::managed_agents_base_dir(app) else {
        return;
    };
    match crate::managed_agents::retention::migrate_legacy_retention_db(
        &base_dir,
        &scope.db_path,
        &scope.owner_keys.public_key().to_hex(),
    ) {
        Ok(0) => {}
        Ok(copied) => {
            eprintln!("buzz-desktop: adopted {copied} legacy retained event(s) into this community")
        }
        Err(error) => eprintln!("buzz-desktop: legacy retention migration failed: {error}"),
    }
}

#[derive(Deserialize)]
struct RelayInfoIcon {
    #[serde(default)]
    icon: Option<String>,
}

/// Fetch a relay's workspace icon from its NIP-11 relay information document.
///
/// Works for any workspace (active or not) with a plain unauthenticated HTTP
/// GET — no WebSocket session needed. Returns `None` when the relay has no
/// icon set, is unreachable, or serves a malformed document: the rail falls
/// back to initials in all three cases.
#[tauri::command]
pub async fn fetch_workspace_icon(
    relay_url: String,
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
    let http_url = relay::relay_http_base_url(&relay_url);
    let Ok(response) = state
        .http_client
        .get(&http_url)
        .header("Accept", "application/nostr+json")
        .send()
        .await
    else {
        return Ok(None);
    };
    if !response.status().is_success() {
        return Ok(None);
    }
    let doc = response
        .json::<RelayInfoIcon>()
        .await
        .unwrap_or(RelayInfoIcon { icon: None });
    Ok(doc.icon.filter(|icon| !icon.is_empty()))
}

#[derive(Serialize)]
pub struct ActiveWorkspaceInfo {
    relay_url: String,
    pubkey: String,
}

/// Returns the current active workspace info (relay URL + pubkey).
#[tauri::command]
pub fn get_active_workspace(state: State<'_, AppState>) -> Result<ActiveWorkspaceInfo, String> {
    let keys = state.keys.lock().map_err(|e| e.to_string())?;
    let relay_url = relay::relay_ws_url_with_override(&state);
    Ok(ActiveWorkspaceInfo {
        relay_url,
        pubkey: keys.public_key().to_hex(),
    })
}

/// Validate a candidate `repos_dir` without mutating the filesystem.
///
/// The Add/Edit workspace dialogs call this on submit to block Save on a bad
/// path, so a typo never reaches `apply_workspace`. Reuses the same
/// `validate_repos_dir` the boot/apply path uses — one source of truth for
/// "what's a valid repos dir". An empty/whitespace value clears the override
/// and is valid. `Err` carries the human-readable reason for inline display.
#[tauri::command]
pub async fn validate_repos_dir(dir: String) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        let trimmed = dir.trim();
        if trimmed.is_empty() {
            return Ok(());
        }
        let nest = nest_dir().ok_or("cannot resolve home directory for nest")?;
        crate::managed_agents::validate_repos_dir(&nest, trimmed).map(|_| ())
    })
    .await
    .map_err(|e| format!("spawn_blocking failed: {e}"))?
}

#[derive(Default)]
pub(crate) struct WorkspaceTransitionState {
    generation: AtomicU64,
    commit: Mutex<()>,
    /// Provider deployments are externally last-write-wins. Serialize the
    /// complete reconcile so a newer transition waits for stale network work
    /// to drain, rechecks ownership, and is guaranteed to publish last.
    provider_reconcile: tokio::sync::Mutex<()>,
}

#[derive(Clone, Copy)]
pub(crate) struct WorkspaceTransitionOwner<'a> {
    transition: &'a WorkspaceTransitionState,
    generation: u64,
}

impl<'a> WorkspaceTransitionOwner<'a> {
    pub(crate) fn is_current(self) -> bool {
        self.transition.is_current(self.generation)
    }

    pub(crate) fn lock_if_current(self) -> Option<std::sync::MutexGuard<'a, ()>> {
        let guard = self
            .transition
            .commit
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        self.is_current().then_some(guard)
    }

    pub(crate) fn while_current<T>(self, action: impl FnOnce() -> T) -> Option<T> {
        let _guard = self.lock_if_current()?;
        Some(action())
    }

    #[cfg(any(feature = "mesh-llm", test))]
    pub(crate) fn take_if_current_and<T>(
        self,
        slot: &mut Option<T>,
        should_take: impl FnOnce(&T) -> bool,
    ) -> Option<T> {
        let _guard = self.lock_if_current()?;
        if slot.as_ref().is_some_and(should_take) {
            slot.take()
        } else {
            None
        }
    }
}

impl WorkspaceTransitionState {
    fn claim_next(&self) -> u64 {
        let _commit_guard = self.commit.lock().unwrap_or_else(|e| e.into_inner());
        self.generation.fetch_add(1, Ordering::AcqRel) + 1
    }

    fn is_current(&self, generation: u64) -> bool {
        self.generation.load(Ordering::Acquire) == generation
    }

    pub(crate) fn current_generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    fn owner(&self, generation: u64) -> WorkspaceTransitionOwner<'_> {
        WorkspaceTransitionOwner {
            transition: self,
            generation,
        }
    }

    async fn reconcile_provider_if_current<F, Fut, T>(
        &self,
        generation: u64,
        reconcile: F,
    ) -> Option<T>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = T>,
    {
        let _guard = self.provider_reconcile.lock().await;
        if !self.is_current(generation) {
            return None;
        }
        Some(reconcile().await)
    }

    fn restore_pending_for_current(&self, generation: u64, pending: &AtomicBool) -> bool {
        self.is_current(generation) && pending.load(Ordering::Acquire)
    }

    fn complete_restore_if_current(&self, generation: u64, pending: &AtomicBool) {
        let _commit_guard = self.commit.lock().unwrap_or_else(|e| e.into_inner());
        if self.is_current(generation) {
            pending.store(false, Ordering::Release);
        }
    }
}

fn workspace_transition_is_current(state: &AppState, generation: u64) -> bool {
    state.workspace_transition.is_current(generation)
}

/// Allocate process-lifetime ownership for a frontend workspace transition
/// before it begins async teardown. The native authority survives webview and
/// React remounts, so callers cannot restart generation numbering at one.
#[tauri::command]
pub fn claim_workspace_transition(state: State<'_, AppState>) -> u64 {
    state.workspace_transition.claim_next()
}

/// Apply a workspace's configuration to the backend session.
///
/// Called by the frontend on app init (after reload) to configure the
/// Tauri backend with the selected workspace's relay URL, keys, and repos
/// directory.
///
/// A bad `repos_dir` is non-fatal: relay/keys always apply (the relay is the
/// active workspace's own choice — orthogonal to the filesystem repos dir),
/// the bad value is NOT persisted (so the next boot starts clean), the
/// `REPOS` symlink is skipped (REPOS stays a real dir), a `repos-dir-error`
/// event surfaces the reason, and the command returns `Ok`. The dialogs
/// already block a bad path at Save (`validate_repos_dir`); this fallback only
/// catches a value that went bad after save (deleted dir, unmounted volume).
#[tauri::command]
pub async fn apply_workspace(
    relay_url: String,
    nsec: Option<String>,
    repos_dir: Option<String>,
    agent_managed_profiles: Option<bool>,
    transition_generation: u64,
    app: AppHandle,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    // The token was allocated by `claim_workspace_transition`. An apply may
    // use it only while it remains the newest process-lifetime intent.
    if !workspace_transition_is_current(&state, transition_generation) {
        return Ok(());
    }

    let restore_app = app.clone();
    let true = tokio::task::spawn_blocking(move || {
        let state = app.state::<AppState>();

        // ── Validate before mutating ──────────────────────────────────────────
        let parsed_keys = match nsec.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(nsec_trimmed) => {
                Some(Keys::parse(nsec_trimmed).map_err(|e| format!("invalid nsec: {e}"))?)
            }
            None => None,
        };

        // Decide the effective repos_dir from the candidate. A bad path does NOT
        // reject — it is treated as if no override were set: relay/keys still
        // apply, the bad value is not persisted, and a `repos-dir-error` surfaces
        // the reason. Persisting a bad path would make every later boot read it,
        // fail to resolve the symlink, and silently skip agent restore. One
        // validate (inside `effective_repos_dir`) drives both the emit and the
        // persisted value. `nest` is resolved softly: when absent there is nothing
        // to persist or symlink, and relay/keys must still apply unconditionally.
        let nest = nest_dir();
        let effective_repos_dir = match nest.as_deref() {
            Some(nest) => match effective_repos_dir(nest, repos_dir.as_deref()) {
                Ok(value) => value,
                Err(error) => {
                    let _ = app.emit("repos-dir-error", error);
                    None
                }
            },
            None => None,
        };

        // Commit all synchronous state and filesystem changes under one lock.
        // A newer command claims its generation before waiting here, so this
        // final check prevents a superseded apply from mutating any authority.
        let _commit_guard = state
            .workspace_transition
            .commit
            .lock()
            .map_err(|e| e.to_string())?;
        if !workspace_transition_is_current(&state, transition_generation) {
            return Ok::<bool, String>(false);
        }

        // ── Apply all state changes (nothing below can fail) ──────────────────
        {
            let mut override_guard = state.relay_url_override.lock().map_err(|e| e.to_string())?;
            *override_guard = Some(relay_url);
        }
        // Reset the Rust-side admission gate when switching workspace/community,
        // matching `resetRateLimitGate()` on the TS side (useCommunityInit.ts:38).
        crate::relay_admission::reset_gate_for_workspace_change();

        if let Some(keys) = parsed_keys {
            let mut keys_guard = state.keys.lock().map_err(|e| e.to_string())?;
            *keys_guard = keys;
        }

        // Keep the backend-side reconcile guard aligned with the frontend
        // experiment before launch-time restore can spawn any agents. Missing
        // means the stable behavior: desktop remains authoritative.
        state
            .managed_agent_profile_reconcile_enabled
            .store(!agent_managed_profiles.unwrap_or(false), Ordering::Release);

        // ── Filesystem side-effect (non-fatal) ────────────────────────────────
        // Persist the *effective* repos_dir (None when the candidate failed
        // validation) for the backend to read at boot, then re-point REPOS to
        // match. Persisting first makes the dotfile authoritative even if the
        // symlink apply fails here (e.g. a non-empty real REPOS): the next boot
        // reads the persisted value and resolves the symlink before any agent can
        // clone into REPOS. A bad candidate persists `None`, so the next boot is
        // clean and agent restore proceeds. Failure of either must NOT fail the
        // command — relay/keys are already applied. Surface symlink errors via
        // `repos-dir-error`.
        if let Some(nest) = nest.as_deref() {
            if let Err(error) = write_persisted_repos_dir(nest, effective_repos_dir.as_deref()) {
                eprintln!("buzz-desktop: persist repos dir failed: {error}");
            }
            if let Err(error) = ensure_repos_symlink(nest, effective_repos_dir.as_deref()) {
                eprintln!("buzz-desktop: repos dir setup failed: {error}");
                let _ = app.emit("repos-dir-error", error);
            }
        }

        try_regenerate_nest(&app);

        Ok::<bool, String>(true)
    })
    .await
    .map_err(|e| format!("spawn_blocking failed: {e}"))??
    else {
        return Ok(());
    };

    let state = restore_app.state::<AppState>();
    if !workspace_transition_is_current(&state, transition_generation) {
        return Ok(());
    }
    let Some(reconcile_result) = state
        .workspace_transition
        .reconcile_provider_if_current(transition_generation, || {
            super::agents::provider_access::reconcile_on_workspace_apply(&restore_app, &state)
        })
        .await
    else {
        return Ok(());
    };
    reconcile_result?;
    if !workspace_transition_is_current(&state, transition_generation) {
        return Ok(());
    }

    // Backfill this exact relay+owner scope only after the workspace has been
    // applied. Running at process boot would target the fallback relay and
    // collapse every community into one pending-event store.
    match crate::managed_agents::retention::active_retention_scope(&restore_app, &state) {
        Ok(scope) => {
            // Adopt whatever the pre-scoping release left queued in the global
            // retention database BEFORE the scoped reconcile and flush run, so
            // stranded tombstones and archive requests publish on this boot
            // instead of being abandoned by the storage cutover.
            migrate_legacy_retention_into(&restore_app, &scope);
            crate::event_sync::spawn_event_sync(
                restore_app.clone(),
                scope.owner_keys,
                scope.db_path,
            )
        }
        Err(error) => {
            eprintln!("buzz-desktop: scoped event-sync unavailable after workspace apply: {error}");
        }
    }

    let restore_pending = state
        .workspace_transition
        .restore_pending_for_current(transition_generation, &state.managed_agent_restore_pending);

    // The coordinator starts before React applies the selected workspace, so
    // its startup publication may have used the fallback relay and placeholder
    // identity. Correct it off the command path so an unavailable relay cannot
    // hold the frontend on its loading gate. On initial launch, restore MeshLLM
    // first so a slow stopped-status request cannot overwrite a newly restored
    // serving status, then restore managed agents after the admission identity
    // has been published (or the bounded publication attempt has timed out).
    #[cfg(feature = "mesh-llm")]
    {
        let app = restore_app.clone();
        tauri::async_runtime::spawn(async move {
            let state = app.state::<AppState>();
            if !workspace_transition_is_current(&state, transition_generation) {
                return;
            }
            if restore_pending {
                if let Err(error) = crate::commands::mesh_llm::restore_mesh_sharing(
                    &app,
                    &state,
                    Some(state.workspace_transition.owner(transition_generation)),
                )
                .await
                {
                    eprintln!("buzz-desktop: failed to restore Share Compute: {error}");
                }
            }
            crate::mesh_llm::publish_current_status_once(&app, "workspace apply").await;
            if !workspace_transition_is_current(&state, transition_generation) {
                return;
            }
            if restore_pending {
                match restore_managed_agents_on_launch(
                    &app,
                    &state.shutdown_started,
                    state.workspace_transition.owner(transition_generation),
                )
                .await
                {
                    Ok(()) => state.workspace_transition.complete_restore_if_current(
                        transition_generation,
                        &state.managed_agent_restore_pending,
                    ),
                    Err(error) => {
                        eprintln!("buzz-desktop: failed to restore managed agents: {error}");
                    }
                }
            }
        });
    }

    #[cfg(not(feature = "mesh-llm"))]
    if restore_pending {
        let app = restore_app.clone();
        tauri::async_runtime::spawn(async move {
            let state = app.state::<AppState>();
            if !workspace_transition_is_current(&state, transition_generation) {
                return;
            }
            match restore_managed_agents_on_launch(
                &app,
                &state.shutdown_started,
                state.workspace_transition.owner(transition_generation),
            )
            .await
            {
                Ok(()) => state.workspace_transition.complete_restore_if_current(
                    transition_generation,
                    &state.managed_agent_restore_pending,
                ),
                Err(error) => {
                    eprintln!("buzz-desktop: failed to restore managed agents: {error}");
                }
            }
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::WorkspaceTransitionState;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn workspace_claims_remain_monotonic_across_frontend_epochs() {
        let transition = WorkspaceTransitionState::default();
        let first_mount = transition.claim_next();
        let first_mount_switch = transition.claim_next();
        assert_eq!(first_mount, 1);
        assert_eq!(first_mount_switch, 2);
        assert!(!transition.is_current(first_mount));
        assert!(transition.is_current(first_mount_switch));

        // A recreated frontend asks native state for a fresh token rather than
        // restarting its own counter at one.
        let remounted_frontend = transition.claim_next();
        assert_eq!(remounted_frontend, 3);
        assert!(!transition.is_current(first_mount_switch));
        assert!(transition.is_current(remounted_frontend));
    }

    #[test]
    fn superseded_restore_does_not_consume_launch_pending() {
        let transition = WorkspaceTransitionState::default();
        let pending = AtomicBool::new(true);
        let first = transition.claim_next();
        assert!(transition.restore_pending_for_current(first, &pending));

        let winner = transition.claim_next();
        transition.complete_restore_if_current(first, &pending);
        assert!(pending.load(Ordering::Acquire));
        assert!(transition.restore_pending_for_current(winner, &pending));

        transition.complete_restore_if_current(winner, &pending);
        assert!(!pending.load(Ordering::Acquire));
    }

    #[test]
    fn supersession_during_managed_agent_restore_blocks_spawn_commit() {
        let transition = WorkspaceTransitionState::default();
        let stale = transition.claim_next();
        let stale_owner = transition.owner(stale);
        assert!(stale_owner.is_current());

        let winner = transition.claim_next();
        let mut installed = false;
        assert_eq!(stale_owner.while_current(|| installed = true), None);
        assert!(!installed);
        assert!(transition.owner(winner).is_current());
    }

    #[test]
    fn stale_owner_cannot_take_winners_mesh_runtime() {
        let transition = WorkspaceTransitionState::default();
        let stale_owner = transition.owner(transition.claim_next());
        transition.claim_next();
        let mut runtime = Some("winner");

        assert_eq!(
            stale_owner.take_if_current_and(&mut runtime, |_| true),
            None
        );
        assert_eq!(runtime, Some("winner"));
    }

    #[test]
    fn owner_superseded_while_waiting_cannot_take_winners_mesh_runtime() {
        use std::sync::Arc;

        let transition = Arc::new(WorkspaceTransitionState::default());
        let stale_owner = transition.owner(transition.claim_next());
        let held = transition.commit.lock().unwrap();
        let transition_for_claim = transition.clone();
        let claim = std::thread::spawn(move || transition_for_claim.claim_next());
        drop(held);
        let winner = claim.join().unwrap();
        let mut runtime = Some("winner");

        assert_eq!(
            stale_owner.take_if_current_and(&mut runtime, |_| true),
            None
        );
        assert_eq!(runtime, Some("winner"));
        assert!(transition.owner(winner).is_current());
    }

    #[tokio::test]
    async fn newer_provider_reconcile_runs_last_after_delayed_stale_request() {
        use std::sync::Arc;
        use tokio::sync::Notify;

        let transition = Arc::new(WorkspaceTransitionState::default());
        let stale_generation = transition.claim_next();
        let stale_started = Arc::new(Notify::new());
        let release_stale = Arc::new(Notify::new());
        let writes = Arc::new(tokio::sync::Mutex::new(Vec::new()));

        let stale_task = {
            let transition = transition.clone();
            let stale_started = stale_started.clone();
            let release_stale = release_stale.clone();
            let writes = writes.clone();
            tokio::spawn(async move {
                transition
                    .reconcile_provider_if_current(stale_generation, || async move {
                        stale_started.notify_one();
                        release_stale.notified().await;
                        writes.lock().await.push("stale");
                    })
                    .await
            })
        };
        stale_started.notified().await;

        let winner_generation = transition.claim_next();
        let winner_task = {
            let transition = transition.clone();
            let writes = writes.clone();
            tokio::spawn(async move {
                transition
                    .reconcile_provider_if_current(winner_generation, || async move {
                        writes.lock().await.push("winner");
                    })
                    .await
            })
        };
        release_stale.notify_one();
        stale_task.await.unwrap();
        winner_task.await.unwrap();

        assert_eq!(*writes.lock().await, vec!["stale", "winner"]);
    }
}
