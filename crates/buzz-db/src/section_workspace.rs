//! Normalized persistence for owner-scoped channel-section workspaces.

use buzz_core::section_workspace::{
    canonical_json, validate_command_tags, SectionWorkspaceGrantCommand, SectionWorkspaceImport,
    SectionWorkspaceReaderEnvelope, SectionWorkspaceRevokeCommand, SectionWorkspaceRole,
    SectionWorkspaceRotatedSection, MAX_ENCRYPTED_METADATA_BYTES, MAX_GRANTS,
    MAX_KEY_ENVELOPE_BYTES,
};
use buzz_core::CommunityId;
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, QueryBuilder, Row};
use uuid::Uuid;

use crate::{DbError, Result};

/// Result of an idempotent revision-zero import.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportOutcome {
    /// This transaction created the canonical migrated workspace.
    Imported {
        /// Canonical workspace revision.
        revision: i64,
    },
    /// The exact action and command hash had already committed.
    AlreadyApplied {
        /// Previously committed canonical workspace revision.
        revision: i64,
    },
}

/// Pairwise key envelope installed for one authorized reader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyEnvelopeUpdate {
    /// Reader pubkey.
    pub reader_pubkey: Vec<u8>,
    /// Opaque pairwise-wrapped content key.
    pub envelope: String,
}

/// Re-encrypted section metadata supplied during content-key rotation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RotatedSectionMetadata {
    /// Existing section UUID.
    pub section_id: Uuid,
    /// Label encrypted under the new content key.
    pub encrypted_label: String,
    /// Optional icon encrypted under the new content key.
    pub encrypted_icon: Option<String>,
}

/// Result of an idempotent grant or revoke transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrantMutationOutcome {
    /// This transaction changed canonical state.
    Applied {
        /// New workspace revision.
        revision: i64,
    },
    /// The exact action had already committed.
    AlreadyApplied {
        /// Previously committed workspace revision.
        revision: i64,
    },
}

/// Current authorization result for one reader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceAccess {
    /// The workspace owner has implicit full access.
    Owner,
    /// An active explicit grant.
    Granted(SectionWorkspaceRole),
}

/// One encrypted section in a current projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectedSection {
    /// Stable section UUID.
    pub id: Uuid,
    /// Zero-based display order.
    pub rank: i32,
    /// Opaque authenticated label ciphertext.
    pub encrypted_label: String,
    /// Optional opaque authenticated icon ciphertext.
    pub encrypted_icon: Option<String>,
}

/// One channel assignment in a current projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectedAssignment {
    /// Channel UUID.
    pub channel_id: Uuid,
    /// Destination section UUID.
    pub section_id: Uuid,
    /// Per-assignment revision.
    pub revision: i64,
}

/// Durable marker proving which legacy store was imported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionWorkspaceMigrationMarker {
    /// Legacy kind-30078 event id.
    pub source_event_id: Vec<u8>,
    /// SHA-256 of the canonical legacy plaintext.
    pub source_hash: Vec<u8>,
}

/// Canonical read projection returned only after an authorization check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionWorkspaceProjection {
    /// Workspace owner.
    pub owner_pubkey: Vec<u8>,
    /// Monotonic workspace revision.
    pub revision: i64,
    /// Monotonic layout revision.
    pub layout_revision: i64,
    /// Content-key epoch for encrypted metadata.
    pub key_epoch: i64,
    /// Legacy source event id recorded by migration.
    pub migration_source_event_id: Vec<u8>,
    /// Legacy plaintext hash recorded by migration.
    pub migration_source_hash: Vec<u8>,
    /// Pairwise key envelope for the authenticated reader.
    pub reader_key_envelope: String,
    /// Ordered active sections.
    pub sections: Vec<ProjectedSection>,
    /// Current channel assignments.
    pub assignments: Vec<ProjectedAssignment>,
}

/// One active delegate grant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionWorkspaceGrant {
    /// Delegate pubkey.
    pub actor_pubkey: Vec<u8>,
    /// Granted role.
    pub role: SectionWorkspaceRole,
    /// Grant timestamp.
    pub granted_at: DateTime<Utc>,
}

/// Database adapter for section workspace transactions.
#[derive(Clone)]
pub struct SectionWorkspaceStore {
    pool: PgPool,
}

impl SectionWorkspaceStore {
    /// Construct from the writer pool used by [`crate::Db`].
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Atomically import one legacy store at workspace revision zero.
    ///
    /// `actor_pubkey` must equal `owner_pubkey`. `signed_event` must be a valid
    /// actor-authored import event and is preserved for audit/rebuild, while `command_hash` is the
    /// SHA-256 of the canonical command body and binds `action_id` retries to
    /// one exact operation.
    #[allow(clippy::too_many_arguments)]
    pub async fn import_v1(
        &self,
        community: CommunityId,
        owner_pubkey: &[u8],
        actor_pubkey: &[u8],
        signed_event: &nostr::Event,
        command_hash: &[u8; 32],
        import: &SectionWorkspaceImport,
    ) -> Result<ImportOutcome> {
        validate_pubkey(owner_pubkey)?;
        validate_pubkey(actor_pubkey)?;
        if actor_pubkey != owner_pubkey {
            return Err(DbError::AccessDenied(
                "only the workspace owner may import legacy sections".into(),
            ));
        }
        let (parsed_import, signed_event_json, derived_hash) =
            validate_command::<SectionWorkspaceImport>(
                signed_event,
                actor_pubkey,
                owner_pubkey,
                None,
                buzz_core::kind::KIND_SECTION_WORKSPACE_IMPORT,
            )?;
        parsed_import
            .validate()
            .map_err(|error| DbError::InvalidData(error.to_string()))?;
        if &parsed_import != import || &derived_hash != command_hash {
            return Err(DbError::InvalidData(
                "signed command does not match import mutation or hash".into(),
            ));
        }

        let source_event_id = decode_hex_32(&import.source_event_id, "source_event_id")?;
        let source_hash = decode_hex_32(&import.source_hash, "source_hash")?;
        let mut tx = self.pool.begin().await?;

        sqlx::query(
            "INSERT INTO section_workspaces (community_id, owner_pubkey) VALUES ($1, $2) \
             ON CONFLICT (community_id, owner_pubkey) DO NOTHING",
        )
        .bind(community.as_uuid())
        .bind(owner_pubkey)
        .execute(&mut *tx)
        .await?;

        let workspace = sqlx::query(
            "SELECT revision, migration_source_event_id, migration_source_hash \
             FROM section_workspaces WHERE community_id = $1 AND owner_pubkey = $2 FOR UPDATE",
        )
        .bind(community.as_uuid())
        .bind(owner_pubkey)
        .fetch_one(&mut *tx)
        .await?;
        let revision: i64 = workspace.try_get("revision")?;
        if revision != 0 {
            let existing = sqlx::query(
                "SELECT command_hash, resulting_revision FROM section_actions \
                 WHERE community_id = $1 AND owner_pubkey = $2 AND action_id = $3",
            )
            .bind(community.as_uuid())
            .bind(owner_pubkey)
            .bind(import.action_id)
            .fetch_optional(&mut *tx)
            .await?;
            if let Some(existing) = existing {
                let existing_hash: Vec<u8> = existing.try_get("command_hash")?;
                let resulting_revision: i64 = existing.try_get("resulting_revision")?;
                if existing_hash.as_slice() == command_hash {
                    tx.commit().await?;
                    return Ok(ImportOutcome::AlreadyApplied {
                        revision: resulting_revision,
                    });
                }
                return Err(DbError::InvalidData(
                    "action_id is already bound to a different command".into(),
                ));
            }
            return Err(DbError::InvalidData(
                "workspace has already been migrated".into(),
            ));
        }

        if !import.assignments.is_empty() {
            let channel_ids = import
                .assignments
                .iter()
                .map(|assignment| assignment.channel_id)
                .collect::<Vec<_>>();
            let count: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM channels \
                 WHERE community_id = $1 AND id = ANY($2) AND deleted_at IS NULL",
            )
            .bind(community.as_uuid())
            .bind(&channel_ids)
            .fetch_one(&mut *tx)
            .await?;
            if count as usize != channel_ids.len() {
                return Err(DbError::InvalidData(
                    "import contains a missing or deleted channel".into(),
                ));
            }
        }

        if !import.sections.is_empty() {
            let mut query = QueryBuilder::new(
                "INSERT INTO sections (community_id, owner_pubkey, section_id, \
                 encrypted_label, encrypted_icon, rank, revision) ",
            );
            query.push_values(&import.sections, |mut row, section| {
                row.push_bind(community.as_uuid())
                    .push_bind(owner_pubkey)
                    .push_bind(section.id)
                    .push_bind(&section.encrypted_label)
                    .push_bind(&section.encrypted_icon)
                    .push_bind(section.rank as i32)
                    .push_bind(1_i64);
            });
            query.build().execute(&mut *tx).await?;
        }

        if !import.assignments.is_empty() {
            let mut query = QueryBuilder::new(
                "INSERT INTO section_assignments (community_id, owner_pubkey, channel_id, \
                 section_id, revision, updated_by) ",
            );
            query.push_values(&import.assignments, |mut row, assignment| {
                row.push_bind(community.as_uuid())
                    .push_bind(owner_pubkey)
                    .push_bind(assignment.channel_id)
                    .push_bind(assignment.section_id)
                    .push_bind(1_i64)
                    .push_bind(owner_pubkey);
            });
            query.build().execute(&mut *tx).await?;
        }

        sqlx::query(
            "INSERT INTO section_key_envelopes \
             (community_id, owner_pubkey, reader_pubkey, key_epoch, envelope) \
             VALUES ($1, $2, $2, $3, $4)",
        )
        .bind(community.as_uuid())
        .bind(owner_pubkey)
        .bind(import.key_epoch as i64)
        .bind(&import.owner_key_envelope)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            "UPDATE section_workspaces SET revision = 1, layout_revision = 1, key_epoch = $3, \
             migrated_at = now(), migration_source_event_id = $4, migration_source_hash = $5, \
             updated_at = now() WHERE community_id = $1 AND owner_pubkey = $2",
        )
        .bind(community.as_uuid())
        .bind(owner_pubkey)
        .bind(import.key_epoch as i64)
        .bind(source_event_id)
        .bind(source_hash)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            "INSERT INTO section_actions (community_id, owner_pubkey, action_id, actor_pubkey, \
             command_kind, command_event_id, signed_event_json, command_hash, resulting_revision) \
             VALUES ($1, $2, $3, $2, 'import_v1', $4, $5, $6, 1)",
        )
        .bind(community.as_uuid())
        .bind(owner_pubkey)
        .bind(import.action_id)
        .bind(signed_event.id.as_bytes().as_slice())
        .bind(&signed_event_json)
        .bind(command_hash.as_slice())
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(ImportOutcome::Imported { revision: 1 })
    }

    /// Owner-only grant or role update with a current-epoch key envelope.
    #[allow(clippy::too_many_arguments)]
    pub async fn grant(
        &self,
        community: CommunityId,
        owner_pubkey: &[u8],
        actor_pubkey: &[u8],
        target_pubkey: &[u8],
        role: SectionWorkspaceRole,
        expected_key_epoch: i64,
        key_envelope: &str,
        action_id: Uuid,
        signed_event: &nostr::Event,
        command_hash: &[u8; 32],
    ) -> Result<GrantMutationOutcome> {
        validate_owner_mutation(owner_pubkey, actor_pubkey, action_id)?;
        let supplied_command = SectionWorkspaceGrantCommand {
            version: buzz_core::section_workspace::SECTION_WORKSPACE_VERSION,
            action_id,
            actor_pubkey: hex::encode(target_pubkey),
            role,
            key_epoch: u64::try_from(expected_key_epoch)
                .map_err(|_| DbError::InvalidData("invalid key epoch".into()))?,
            key_envelope: key_envelope.to_owned(),
        };
        let (parsed_command, signed_event_json, derived_hash) =
            validate_command::<SectionWorkspaceGrantCommand>(
                signed_event,
                actor_pubkey,
                owner_pubkey,
                Some(supplied_command.actor_pubkey.as_str()),
                buzz_core::kind::KIND_SECTION_WORKSPACE_GRANT,
            )?;
        parsed_command
            .validate()
            .map_err(|error| DbError::InvalidData(error.to_string()))?;
        if parsed_command != supplied_command || &derived_hash != command_hash {
            return Err(DbError::InvalidData(
                "signed command does not match grant mutation or hash".into(),
            ));
        }
        validate_pubkey(target_pubkey)?;
        if target_pubkey == owner_pubkey {
            return Err(DbError::InvalidData(
                "owner access is implicit and cannot be granted".into(),
            ));
        }
        validate_envelope(key_envelope)?;
        let mut tx = self.pool.begin().await?;
        let (revision, key_epoch) =
            lock_migrated_workspace(&mut tx, community, owner_pubkey).await?;
        if let Some(outcome) =
            existing_action(&mut tx, community, owner_pubkey, action_id, command_hash).await?
        {
            tx.commit().await?;
            return Ok(outcome);
        }
        if expected_key_epoch != key_epoch {
            return Err(DbError::InvalidData(
                "grant key epoch does not match current workspace epoch".into(),
            ));
        }
        let grant_count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM section_grants WHERE community_id = $1 AND owner_pubkey = $2 \
             AND status = 'active'",
        )
        .bind(community.as_uuid())
        .bind(owner_pubkey)
        .fetch_one(&mut *tx)
        .await?;
        let target_is_active: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM section_grants WHERE community_id = $1 \
             AND owner_pubkey = $2 AND actor_pubkey = $3 AND status = 'active')",
        )
        .bind(community.as_uuid())
        .bind(owner_pubkey)
        .bind(target_pubkey)
        .fetch_one(&mut *tx)
        .await?;
        if !target_is_active && grant_count >= MAX_GRANTS as i64 {
            return Err(DbError::InvalidData(format!(
                "section workspace grants exceed maximum {MAX_GRANTS}"
            )));
        }
        let next_revision = revision
            .checked_add(1)
            .ok_or_else(|| DbError::InvalidData("workspace revision exhausted".into()))?;
        sqlx::query(
            "INSERT INTO section_grants (community_id, owner_pubkey, actor_pubkey, role, status, \
             granted_by, granted_at, revoked_at) VALUES ($1, $2, $3, $4, 'active', $2, now(), NULL) \
             ON CONFLICT (community_id, owner_pubkey, actor_pubkey) DO UPDATE SET \
             role = EXCLUDED.role, status = 'active', granted_by = EXCLUDED.granted_by, \
             granted_at = now(), revoked_at = NULL",
        )
        .bind(community.as_uuid()).bind(owner_pubkey).bind(target_pubkey)
        .bind(role.as_str()).execute(&mut *tx).await?;
        sqlx::query(
            "INSERT INTO section_key_envelopes \
             (community_id, owner_pubkey, reader_pubkey, key_epoch, envelope) \
             VALUES ($1, $2, $3, $4, $5) \
             ON CONFLICT (community_id, owner_pubkey, reader_pubkey, key_epoch) \
             DO UPDATE SET envelope = EXCLUDED.envelope, created_at = now()",
        )
        .bind(community.as_uuid())
        .bind(owner_pubkey)
        .bind(target_pubkey)
        .bind(key_epoch)
        .bind(key_envelope)
        .execute(&mut *tx)
        .await?;
        finish_grant_action(
            &mut tx,
            community,
            owner_pubkey,
            action_id,
            actor_pubkey,
            "grant",
            signed_event,
            &signed_event_json,
            command_hash,
            next_revision,
        )
        .await?;
        tx.commit().await?;
        Ok(GrantMutationOutcome::Applied {
            revision: next_revision,
        })
    }

    /// Owner-only revoke with atomic content-key rotation.
    ///
    /// Every active section must be represented exactly once under
    /// `new_key_epoch`, and envelopes must cover the owner plus every remaining
    /// active grantee, excluding the revoked reader.
    #[allow(clippy::too_many_arguments)]
    pub async fn revoke_and_rotate(
        &self,
        community: CommunityId,
        owner_pubkey: &[u8],
        actor_pubkey: &[u8],
        target_pubkey: &[u8],
        new_key_epoch: i64,
        sections: &[RotatedSectionMetadata],
        envelopes: &[KeyEnvelopeUpdate],
        action_id: Uuid,
        signed_event: &nostr::Event,
        command_hash: &[u8; 32],
    ) -> Result<GrantMutationOutcome> {
        validate_owner_mutation(owner_pubkey, actor_pubkey, action_id)?;
        let supplied_command = SectionWorkspaceRevokeCommand {
            version: buzz_core::section_workspace::SECTION_WORKSPACE_VERSION,
            action_id,
            actor_pubkey: hex::encode(target_pubkey),
            new_key_epoch: u64::try_from(new_key_epoch)
                .map_err(|_| DbError::InvalidData("invalid key epoch".into()))?,
            sections: sections
                .iter()
                .map(|section| SectionWorkspaceRotatedSection {
                    section_id: section.section_id,
                    encrypted_label: section.encrypted_label.clone(),
                    encrypted_icon: section.encrypted_icon.clone(),
                })
                .collect(),
            envelopes: envelopes
                .iter()
                .map(|envelope| SectionWorkspaceReaderEnvelope {
                    reader_pubkey: hex::encode(&envelope.reader_pubkey),
                    key_envelope: envelope.envelope.clone(),
                })
                .collect(),
        };
        let (parsed_command, signed_event_json, derived_hash) =
            validate_command::<SectionWorkspaceRevokeCommand>(
                signed_event,
                actor_pubkey,
                owner_pubkey,
                Some(supplied_command.actor_pubkey.as_str()),
                buzz_core::kind::KIND_SECTION_WORKSPACE_REVOKE,
            )?;
        parsed_command
            .validate()
            .map_err(|error| DbError::InvalidData(error.to_string()))?;
        if parsed_command != supplied_command || &derived_hash != command_hash {
            return Err(DbError::InvalidData(
                "signed command does not match revoke mutation or hash".into(),
            ));
        }
        validate_pubkey(target_pubkey)?;
        if target_pubkey == owner_pubkey {
            return Err(DbError::InvalidData(
                "owner access cannot be revoked".into(),
            ));
        }
        let mut tx = self.pool.begin().await?;
        let (revision, key_epoch) =
            lock_migrated_workspace(&mut tx, community, owner_pubkey).await?;
        if let Some(outcome) =
            existing_action(&mut tx, community, owner_pubkey, action_id, command_hash).await?
        {
            tx.commit().await?;
            return Ok(outcome);
        }
        if new_key_epoch
            != key_epoch
                .checked_add(1)
                .ok_or_else(|| DbError::InvalidData("key epoch exhausted".into()))?
        {
            return Err(DbError::InvalidData(
                "new key epoch must be current epoch + 1".into(),
            ));
        }
        let next_revision = revision
            .checked_add(1)
            .ok_or_else(|| DbError::InvalidData("workspace revision exhausted".into()))?;
        validate_rotation_shape(
            &mut tx,
            community,
            owner_pubkey,
            target_pubkey,
            sections,
            envelopes,
        )
        .await?;
        let affected = sqlx::query(
            "UPDATE section_grants SET status = 'revoked', revoked_at = now() \
             WHERE community_id = $1 AND owner_pubkey = $2 AND actor_pubkey = $3 \
             AND status = 'active'",
        )
        .bind(community.as_uuid())
        .bind(owner_pubkey)
        .bind(target_pubkey)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if affected != 1 {
            return Err(DbError::NotFound("active section workspace grant".into()));
        }
        for section in sections {
            sqlx::query(
                "UPDATE sections SET encrypted_label = $4, encrypted_icon = $5, revision = $6 \
                 WHERE community_id = $1 AND owner_pubkey = $2 AND section_id = $3 \
                 AND deleted_at IS NULL",
            )
            .bind(community.as_uuid())
            .bind(owner_pubkey)
            .bind(section.section_id)
            .bind(&section.encrypted_label)
            .bind(&section.encrypted_icon)
            .bind(next_revision)
            .execute(&mut *tx)
            .await?;
        }
        for envelope in envelopes {
            sqlx::query(
                "INSERT INTO section_key_envelopes \
                 (community_id, owner_pubkey, reader_pubkey, key_epoch, envelope) \
                 VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(community.as_uuid())
            .bind(owner_pubkey)
            .bind(&envelope.reader_pubkey)
            .bind(new_key_epoch)
            .bind(&envelope.envelope)
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query(
            "UPDATE section_workspaces SET revision = $3, key_epoch = $4, updated_at = now() \
             WHERE community_id = $1 AND owner_pubkey = $2",
        )
        .bind(community.as_uuid())
        .bind(owner_pubkey)
        .bind(next_revision)
        .bind(new_key_epoch)
        .execute(&mut *tx)
        .await?;
        finish_grant_action(
            &mut tx,
            community,
            owner_pubkey,
            action_id,
            actor_pubkey,
            "revoke",
            signed_event,
            &signed_event_json,
            command_hash,
            next_revision,
        )
        .await?;
        tx.commit().await?;
        Ok(GrantMutationOutcome::Applied {
            revision: next_revision,
        })
    }

    /// Resolve current read authority. The owner is implicit; delegates require
    /// an active exact-pubkey grant.
    pub async fn access(
        &self,
        community: CommunityId,
        owner_pubkey: &[u8],
        reader_pubkey: &[u8],
    ) -> Result<Option<WorkspaceAccess>> {
        validate_pubkey(owner_pubkey)?;
        validate_pubkey(reader_pubkey)?;
        if owner_pubkey == reader_pubkey {
            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM section_workspaces \
                 WHERE community_id = $1 AND owner_pubkey = $2 AND migrated_at IS NOT NULL)",
            )
            .bind(community.as_uuid())
            .bind(owner_pubkey)
            .fetch_one(&self.pool)
            .await?;
            return Ok(exists.then_some(WorkspaceAccess::Owner));
        }
        let role: Option<String> = sqlx::query_scalar(
            "SELECT role FROM section_grants WHERE community_id = $1 AND owner_pubkey = $2 \
             AND actor_pubkey = $3 AND status = 'active'",
        )
        .bind(community.as_uuid())
        .bind(owner_pubkey)
        .bind(reader_pubkey)
        .fetch_optional(&self.pool)
        .await?;
        role.map(|role| parse_role(&role).map(WorkspaceAccess::Granted))
            .transpose()
    }

    /// Lookup the migration marker without granting access to projection data.
    ///
    /// This is owner-authorized because the marker identifies the owner's
    /// legacy event and plaintext hash. Unmigrated and unauthorized workspaces
    /// both return `None`.
    pub async fn migration_marker(
        &self,
        community: CommunityId,
        owner_pubkey: &[u8],
        reader_pubkey: &[u8],
    ) -> Result<Option<SectionWorkspaceMigrationMarker>> {
        validate_pubkey(owner_pubkey)?;
        validate_pubkey(reader_pubkey)?;
        let mut tx = self.pool.begin().await?;
        // Lock the workspace before resolving the grant. Revoke takes the
        // corresponding UPDATE lock, so an authorized read linearizes wholly
        // before or wholly after revocation rather than leaking a post-revoke
        // marker through a check/use race.
        let row = sqlx::query(
            "SELECT migration_source_event_id, migration_source_hash FROM section_workspaces \
             WHERE community_id = $1 AND owner_pubkey = $2 AND migrated_at IS NOT NULL FOR SHARE",
        )
        .bind(community.as_uuid())
        .bind(owner_pubkey)
        .fetch_optional(&mut *tx)
        .await?;
        let authorized = owner_pubkey == reader_pubkey
            || active_grant_exists(&mut tx, community, owner_pubkey, reader_pubkey).await?;
        if !authorized {
            tx.commit().await?;
            return Ok(None);
        }
        let marker = row
            .map(|row| -> Result<SectionWorkspaceMigrationMarker> {
                Ok(SectionWorkspaceMigrationMarker {
                    source_event_id: row.try_get("migration_source_event_id")?,
                    source_hash: row.try_get("migration_source_hash")?,
                })
            })
            .transpose()?;
        tx.commit().await?;
        Ok(marker)
    }

    /// Fetch the canonical projection for an owner or active delegate.
    pub async fn projection(
        &self,
        community: CommunityId,
        owner_pubkey: &[u8],
        reader_pubkey: &[u8],
    ) -> Result<Option<SectionWorkspaceProjection>> {
        validate_pubkey(owner_pubkey)?;
        validate_pubkey(reader_pubkey)?;
        let mut tx = self.pool.begin().await?;
        // Keep authorization and every projected row under a shared workspace
        // lock. Grant/revoke mutations serialize on the workspace's UPDATE
        // lock, closing the authorization check/use race across these queries.
        let workspace = sqlx::query(
            "SELECT revision, layout_revision, key_epoch, migration_source_event_id, \
             migration_source_hash FROM section_workspaces \
             WHERE community_id = $1 AND owner_pubkey = $2 AND migrated_at IS NOT NULL FOR SHARE",
        )
        .bind(community.as_uuid())
        .bind(owner_pubkey)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(workspace) = workspace else {
            tx.commit().await?;
            return Ok(None);
        };
        let authorized = owner_pubkey == reader_pubkey
            || active_grant_exists(&mut tx, community, owner_pubkey, reader_pubkey).await?;
        if !authorized {
            tx.commit().await?;
            return Ok(None);
        }
        let key_epoch: i64 = workspace.try_get("key_epoch")?;
        let reader_key_envelope: String = sqlx::query_scalar(
            "SELECT envelope FROM section_key_envelopes WHERE community_id = $1 \
             AND owner_pubkey = $2 AND reader_pubkey = $3 AND key_epoch = $4",
        )
        .bind(community.as_uuid())
        .bind(owner_pubkey)
        .bind(reader_pubkey)
        .bind(key_epoch)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| {
            DbError::InvalidData("authorized reader has no current key envelope".into())
        })?;
        let sections = sqlx::query(
            "SELECT section_id, rank, encrypted_label, encrypted_icon FROM sections \
             WHERE community_id = $1 AND owner_pubkey = $2 AND deleted_at IS NULL ORDER BY rank",
        )
        .bind(community.as_uuid())
        .bind(owner_pubkey)
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .map(|row| {
            Ok(ProjectedSection {
                id: row.try_get("section_id")?,
                rank: row.try_get("rank")?,
                encrypted_label: row.try_get("encrypted_label")?,
                encrypted_icon: row.try_get("encrypted_icon")?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
        let assignments = sqlx::query(
            "SELECT channel_id, section_id, revision FROM section_assignments \
             WHERE community_id = $1 AND owner_pubkey = $2 ORDER BY channel_id",
        )
        .bind(community.as_uuid())
        .bind(owner_pubkey)
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .map(|row| {
            Ok(ProjectedAssignment {
                channel_id: row.try_get("channel_id")?,
                section_id: row.try_get("section_id")?,
                revision: row.try_get("revision")?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
        let projection = SectionWorkspaceProjection {
            owner_pubkey: owner_pubkey.to_vec(),
            revision: workspace.try_get("revision")?,
            layout_revision: workspace.try_get("layout_revision")?,
            key_epoch,
            migration_source_event_id: workspace.try_get("migration_source_event_id")?,
            migration_source_hash: workspace.try_get("migration_source_hash")?,
            reader_key_envelope,
            sections,
            assignments,
        };
        tx.commit().await?;
        Ok(Some(projection))
    }
}

async fn active_grant_exists(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    community: CommunityId,
    owner_pubkey: &[u8],
    reader_pubkey: &[u8],
) -> Result<bool> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM section_grants WHERE community_id = $1 \
         AND owner_pubkey = $2 AND actor_pubkey = $3 AND status = 'active')",
    )
    .bind(community.as_uuid())
    .bind(owner_pubkey)
    .bind(reader_pubkey)
    .fetch_one(&mut **tx)
    .await?)
}

async fn lock_migrated_workspace(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    community: CommunityId,
    owner_pubkey: &[u8],
) -> Result<(i64, i64)> {
    let row = sqlx::query(
        "SELECT revision, key_epoch FROM section_workspaces \
         WHERE community_id = $1 AND owner_pubkey = $2 AND migrated_at IS NOT NULL FOR UPDATE",
    )
    .bind(community.as_uuid())
    .bind(owner_pubkey)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| DbError::NotFound("migrated section workspace".into()))?;
    Ok((row.try_get("revision")?, row.try_get("key_epoch")?))
}

async fn existing_action(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    community: CommunityId,
    owner_pubkey: &[u8],
    action_id: Uuid,
    command_hash: &[u8; 32],
) -> Result<Option<GrantMutationOutcome>> {
    let row = sqlx::query(
        "SELECT command_hash, resulting_revision FROM section_actions \
         WHERE community_id = $1 AND owner_pubkey = $2 AND action_id = $3",
    )
    .bind(community.as_uuid())
    .bind(owner_pubkey)
    .bind(action_id)
    .fetch_optional(&mut **tx)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let existing_hash: Vec<u8> = row.try_get("command_hash")?;
    if existing_hash.as_slice() != command_hash {
        return Err(DbError::InvalidData(
            "action_id is already bound to a different command".into(),
        ));
    }
    Ok(Some(GrantMutationOutcome::AlreadyApplied {
        revision: row.try_get("resulting_revision")?,
    }))
}

#[allow(clippy::too_many_arguments)]
async fn finish_grant_action(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    community: CommunityId,
    owner_pubkey: &[u8],
    action_id: Uuid,
    actor_pubkey: &[u8],
    command_kind: &str,
    signed_event: &nostr::Event,
    signed_event_json: &serde_json::Value,
    command_hash: &[u8; 32],
    revision: i64,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO section_actions (community_id, owner_pubkey, action_id, actor_pubkey, \
         command_kind, command_event_id, signed_event_json, command_hash, resulting_revision) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(community.as_uuid())
    .bind(owner_pubkey)
    .bind(action_id)
    .bind(actor_pubkey)
    .bind(command_kind)
    .bind(signed_event.id.as_bytes().as_slice())
    .bind(signed_event_json)
    .bind(command_hash.as_slice())
    .bind(revision)
    .execute(&mut **tx)
    .await?;
    sqlx::query(
        "UPDATE section_workspaces SET revision = $3, updated_at = now() \
         WHERE community_id = $1 AND owner_pubkey = $2",
    )
    .bind(community.as_uuid())
    .bind(owner_pubkey)
    .bind(revision)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn validate_rotation_shape(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    community: CommunityId,
    owner_pubkey: &[u8],
    target_pubkey: &[u8],
    sections: &[RotatedSectionMetadata],
    envelopes: &[KeyEnvelopeUpdate],
) -> Result<()> {
    let active_sections: Vec<Uuid> = sqlx::query_scalar(
        "SELECT section_id FROM sections WHERE community_id = $1 AND owner_pubkey = $2 \
         AND deleted_at IS NULL ORDER BY section_id",
    )
    .bind(community.as_uuid())
    .bind(owner_pubkey)
    .fetch_all(&mut **tx)
    .await?;
    let mut supplied_sections = Vec::with_capacity(sections.len());
    for section in sections {
        validate_ciphertext_db(
            &section.encrypted_label,
            MAX_ENCRYPTED_METADATA_BYTES,
            "label",
        )?;
        if let Some(icon) = &section.encrypted_icon {
            validate_ciphertext_db(icon, MAX_ENCRYPTED_METADATA_BYTES, "icon")?;
        }
        supplied_sections.push(section.section_id);
    }
    supplied_sections.sort_unstable();
    if supplied_sections != active_sections {
        return Err(DbError::InvalidData(
            "rotation must re-encrypt every active section exactly once".into(),
        ));
    }
    let mut required_readers: Vec<Vec<u8>> = sqlx::query_scalar(
        "SELECT actor_pubkey FROM section_grants WHERE community_id = $1 AND owner_pubkey = $2 \
         AND status = 'active' AND actor_pubkey <> $3 ORDER BY actor_pubkey",
    )
    .bind(community.as_uuid())
    .bind(owner_pubkey)
    .bind(target_pubkey)
    .fetch_all(&mut **tx)
    .await?;
    required_readers.push(owner_pubkey.to_vec());
    required_readers.sort();
    let mut supplied_readers = Vec::with_capacity(envelopes.len());
    for envelope in envelopes {
        validate_pubkey(&envelope.reader_pubkey)?;
        validate_envelope(&envelope.envelope)?;
        supplied_readers.push(envelope.reader_pubkey.clone());
    }
    supplied_readers.sort();
    if supplied_readers != required_readers {
        return Err(DbError::InvalidData(
            "rotation envelopes must cover owner and every remaining active grantee exactly once"
                .into(),
        ));
    }
    Ok(())
}

fn validate_command<T>(
    event: &nostr::Event,
    actor_pubkey: &[u8],
    owner_pubkey: &[u8],
    target_pubkey: Option<&str>,
    expected_kind: u32,
) -> Result<(T, serde_json::Value, [u8; 32])>
where
    T: serde::de::DeserializeOwned + serde::Serialize,
{
    if event.kind.as_u16() as u32 != expected_kind {
        return Err(DbError::InvalidData(
            "signed command event has wrong kind".into(),
        ));
    }
    if event.pubkey.to_bytes().as_slice() != actor_pubkey {
        return Err(DbError::AccessDenied(
            "signed command author does not match actor".into(),
        ));
    }
    if !event.verify_id() || !event.verify_signature() {
        return Err(DbError::InvalidData(
            "signed command event has invalid id or signature".into(),
        ));
    }
    let command: T = serde_json::from_str(&event.content)
        .map_err(|error| DbError::InvalidData(format!("invalid command content: {error}")))?;
    let value = serde_json::to_value(&command)
        .map_err(|error| DbError::InvalidData(format!("cannot canonicalize command: {error}")))?;
    let canonical =
        canonical_json(&value).map_err(|error| DbError::InvalidData(error.to_string()))?;
    if event.content != canonical {
        return Err(DbError::InvalidData(
            "command content is not canonical JSON".into(),
        ));
    }
    let action_id = value
        .get("action_id")
        .and_then(serde_json::Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
        .ok_or_else(|| DbError::InvalidData("invalid command action_id".into()))?;
    let tags = event
        .tags
        .iter()
        .map(|tag| tag.as_slice().to_vec())
        .collect::<Vec<_>>();
    validate_command_tags(&tags, &hex::encode(owner_pubkey), target_pubkey, action_id)
        .map_err(|error| DbError::InvalidData(error.to_string()))?;
    let hash: [u8; 32] = Sha256::digest(canonical.as_bytes()).into();
    let signed_event_json = serde_json::to_value(event)
        .map_err(|error| DbError::InvalidData(format!("cannot retain signed command: {error}")))?;
    Ok((command, signed_event_json, hash))
}

fn validate_owner_mutation(owner: &[u8], actor: &[u8], action_id: Uuid) -> Result<()> {
    validate_pubkey(owner)?;
    validate_pubkey(actor)?;
    if owner != actor {
        return Err(DbError::AccessDenied(
            "only the workspace owner may change grants".into(),
        ));
    }
    if action_id.is_nil() {
        return Err(DbError::InvalidData("action_id must not be nil".into()));
    }
    Ok(())
}

fn validate_envelope(envelope: &str) -> Result<()> {
    validate_ciphertext_db(envelope, MAX_KEY_ENVELOPE_BYTES, "key envelope")
}

fn validate_ciphertext_db(value: &str, maximum: usize, field: &str) -> Result<()> {
    if value.is_empty() || value.len() > maximum {
        return Err(DbError::InvalidData(format!("invalid encrypted {field}")));
    }
    Ok(())
}

fn validate_pubkey(pubkey: &[u8]) -> Result<()> {
    if pubkey.len() != 32 {
        return Err(DbError::InvalidData("pubkey must be 32 bytes".into()));
    }
    Ok(())
}

fn decode_hex_32(value: &str, field: &str) -> Result<Vec<u8>> {
    let decoded =
        hex::decode(value).map_err(|_| DbError::InvalidData(format!("invalid {field}")))?;
    if decoded.len() != 32 {
        return Err(DbError::InvalidData(format!("invalid {field}")));
    }
    Ok(decoded)
}

fn parse_role(value: &str) -> Result<SectionWorkspaceRole> {
    match value {
        "viewer" => Ok(SectionWorkspaceRole::Viewer),
        "mover" => Ok(SectionWorkspaceRole::Mover),
        "manager" => Ok(SectionWorkspaceRole::Manager),
        _ => Err(DbError::InvalidData(format!(
            "unknown section workspace role {value:?}"
        ))),
    }
}

const _: () = assert!(MAX_ENCRYPTED_METADATA_BYTES <= 65_535);
const _: () = assert!(MAX_KEY_ENVELOPE_BYTES <= 4_096);

#[cfg(test)]
mod tests {
    use super::*;
    use buzz_core::section_workspace::{
        SectionWorkspaceImportAssignment, SectionWorkspaceImportSection,
    };
    use nostr::{Event, EventBuilder, JsonUtil, Keys, Kind};

    const TEST_DB_URL: &str = "postgres://buzz:buzz_dev@localhost:5432/buzz";

    fn bound_signed_command<T: serde::Serialize>(
        keys: &Keys,
        kind: u32,
        owner: &[u8],
        actor: Option<&[u8]>,
        action_id: Uuid,
        command: &T,
    ) -> (Event, [u8; 32]) {
        use nostr::Tag;
        let value = serde_json::to_value(command).expect("command value");
        let content = canonical_json(&value).expect("canonical command");
        let mut tags = vec![Tag::parse(["p", &hex::encode(owner)]).expect("p tag")];
        if let Some(actor) = actor {
            tags.push(Tag::parse(["actor", &hex::encode(actor)]).expect("actor tag"));
        }
        tags.push(Tag::parse(["action", &action_id.to_string()]).expect("action tag"));
        let event = EventBuilder::new(Kind::Custom(kind as u16), &content)
            .tags(tags)
            .allow_self_tagging()
            .sign_with_keys(keys)
            .expect("sign command");
        let hash = Sha256::digest(content.as_bytes()).into();
        (event, hash)
    }

    fn import_event(
        keys: &Keys,
        owner: &[u8],
        import: &SectionWorkspaceImport,
    ) -> (Event, [u8; 32]) {
        bound_signed_command(
            keys,
            buzz_core::kind::KIND_SECTION_WORKSPACE_IMPORT,
            owner,
            None,
            import.action_id,
            import,
        )
    }

    fn grant_event(
        keys: &Keys,
        owner: &[u8],
        target: &[u8],
        role: SectionWorkspaceRole,
        epoch: i64,
        envelope: &str,
        action_id: Uuid,
    ) -> (Event, [u8; 32]) {
        let command = SectionWorkspaceGrantCommand {
            version: buzz_core::section_workspace::SECTION_WORKSPACE_VERSION,
            action_id,
            actor_pubkey: hex::encode(target),
            role,
            key_epoch: u64::try_from(epoch).unwrap(),
            key_envelope: envelope.into(),
        };
        bound_signed_command(
            keys,
            buzz_core::kind::KIND_SECTION_WORKSPACE_GRANT,
            owner,
            Some(target),
            action_id,
            &command,
        )
    }

    fn revoke_event(
        keys: &Keys,
        owner: &[u8],
        target: &[u8],
        epoch: i64,
        sections: &[RotatedSectionMetadata],
        envelopes: &[KeyEnvelopeUpdate],
        action_id: Uuid,
    ) -> (Event, [u8; 32]) {
        let command = SectionWorkspaceRevokeCommand {
            version: buzz_core::section_workspace::SECTION_WORKSPACE_VERSION,
            action_id,
            actor_pubkey: hex::encode(target),
            new_key_epoch: u64::try_from(epoch).unwrap(),
            sections: sections
                .iter()
                .map(|section| SectionWorkspaceRotatedSection {
                    section_id: section.section_id,
                    encrypted_label: section.encrypted_label.clone(),
                    encrypted_icon: section.encrypted_icon.clone(),
                })
                .collect(),
            envelopes: envelopes
                .iter()
                .map(|envelope| SectionWorkspaceReaderEnvelope {
                    reader_pubkey: hex::encode(&envelope.reader_pubkey),
                    key_envelope: envelope.envelope.clone(),
                })
                .collect(),
        };
        bound_signed_command(
            keys,
            buzz_core::kind::KIND_SECTION_WORKSPACE_REVOKE,
            owner,
            Some(target),
            action_id,
            &command,
        )
    }

    async fn setup_pool() -> PgPool {
        let database_url = std::env::var("BUZZ_TEST_DATABASE_URL")
            .or_else(|_| std::env::var("DATABASE_URL"))
            .unwrap_or_else(|_| TEST_DB_URL.to_owned());
        PgPool::connect(&database_url)
            .await
            .expect("connect to test database")
    }

    async fn make_community(pool: &PgPool) -> CommunityId {
        let id = Uuid::new_v4();
        let host = format!("section-workspace-{}.example", id.simple());
        sqlx::query("INSERT INTO communities (id, host) VALUES ($1, $2)")
            .bind(id)
            .bind(host)
            .execute(pool)
            .await
            .expect("insert test community");
        CommunityId::from_uuid(id)
    }

    async fn make_channel(pool: &PgPool, community: CommunityId, id: Uuid) {
        sqlx::query(
            "INSERT INTO channels (community_id, id, name, created_by) VALUES ($1, $2, $3, $4)",
        )
        .bind(community.as_uuid())
        .bind(id)
        .bind(format!("channel-{}", id.simple()))
        .bind(vec![7_u8; 32])
        .execute(pool)
        .await
        .expect("insert test channel");
    }

    fn import_fixture(
        action_id: Uuid,
        channel_id: Uuid,
        section_id: Uuid,
    ) -> SectionWorkspaceImport {
        SectionWorkspaceImport {
            version: buzz_core::section_workspace::SECTION_WORKSPACE_VERSION,
            action_id,
            source_event_id: "11".repeat(32),
            source_hash: "22".repeat(32),
            key_epoch: 1,
            owner_key_envelope: "owner-envelope-epoch-1".into(),
            sections: vec![SectionWorkspaceImportSection {
                id: section_id,
                rank: 0,
                encrypted_label: "label-epoch-1".into(),
                encrypted_icon: None,
            }],
            assignments: vec![SectionWorkspaceImportAssignment {
                channel_id,
                section_id,
            }],
        }
    }

    #[test]
    fn signed_command_validation_binds_content_tags_hash_author_kind_and_signature() {
        let owner_keys = Keys::generate();
        let owner = owner_keys.public_key().to_bytes();
        let import = import_fixture(Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let (event, expected_hash) = bound_signed_command(
            &owner_keys,
            buzz_core::kind::KIND_SECTION_WORKSPACE_IMPORT,
            &owner,
            None,
            import.action_id,
            &import,
        );
        let (parsed, retained, hash) = validate_command::<SectionWorkspaceImport>(
            &event,
            &owner,
            &owner,
            None,
            buzz_core::kind::KIND_SECTION_WORKSPACE_IMPORT,
        )
        .expect("valid signed command");
        assert_eq!(parsed, import);
        assert_eq!(hash, expected_hash);
        let reconstructed = Event::from_json(retained.to_string()).expect("reconstruct event");
        assert!(reconstructed.verify_id());
        assert!(reconstructed.verify_signature());

        assert!(validate_command::<SectionWorkspaceImport>(
            &event,
            &owner,
            &owner,
            None,
            buzz_core::kind::KIND_SECTION_WORKSPACE_GRANT,
        )
        .is_err());
        assert!(validate_command::<SectionWorkspaceImport>(
            &event,
            &Keys::generate().public_key().to_bytes(),
            &owner,
            None,
            buzz_core::kind::KIND_SECTION_WORKSPACE_IMPORT,
        )
        .is_err());

        let mut tampered_json: serde_json::Value = serde_json::from_str(&event.as_json()).unwrap();
        tampered_json["sig"] = serde_json::Value::String("0".repeat(128));
        let tampered = Event::from_json(tampered_json.to_string()).unwrap();
        assert!(validate_command::<SectionWorkspaceImport>(
            &tampered,
            &owner,
            &owner,
            None,
            buzz_core::kind::KIND_SECTION_WORKSPACE_IMPORT,
        )
        .is_err());
    }

    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn import_is_atomic_and_exact_replay_is_idempotent() {
        let pool = setup_pool().await;
        let community = make_community(&pool).await;
        let owner_keys = Keys::generate();
        let owner = owner_keys.public_key().to_bytes();
        let channel_id = Uuid::new_v4();
        let section_id = Uuid::new_v4();
        make_channel(&pool, community, channel_id).await;
        let store = SectionWorkspaceStore::new(pool.clone());
        let import = import_fixture(Uuid::new_v4(), channel_id, section_id);
        let signed_import = import_event(&owner_keys, &owner, &import);

        assert_eq!(
            store
                .import_v1(
                    community,
                    &owner,
                    &owner,
                    &signed_import.0,
                    &signed_import.1,
                    &import
                )
                .await
                .expect("first import"),
            ImportOutcome::Imported { revision: 1 }
        );
        assert_eq!(
            store
                .import_v1(
                    community,
                    &owner,
                    &owner,
                    &signed_import.0,
                    &signed_import.1,
                    &import
                )
                .await
                .expect("exact retry"),
            ImportOutcome::AlreadyApplied { revision: 1 }
        );

        let projection = store
            .projection(community, &owner, &owner)
            .await
            .expect("owner projection")
            .expect("migrated projection");
        assert_eq!(projection.revision, 1);
        assert_eq!(projection.layout_revision, 1);
        assert_eq!(projection.key_epoch, 1);
        assert_eq!(projection.sections.len(), 1);
        assert_eq!(projection.assignments.len(), 1);
        assert_eq!(projection.reader_key_envelope, "owner-envelope-epoch-1");
        let retained_event: serde_json::Value = sqlx::query_scalar(
            "SELECT signed_event_json FROM section_actions WHERE community_id = $1 \
             AND owner_pubkey = $2 AND action_id = $3",
        )
        .bind(community.as_uuid())
        .bind(owner)
        .bind(import.action_id)
        .fetch_one(&pool)
        .await
        .expect("load durable signed command");
        let retained_event = Event::from_json(retained_event.to_string())
            .expect("reconstruct retained signed command");
        assert!(retained_event.verify_id());
        assert!(retained_event.verify_signature());
        assert_eq!(retained_event.pubkey, owner_keys.public_key());
        assert_eq!(
            retained_event.kind,
            Kind::Custom(buzz_core::kind::KIND_SECTION_WORKSPACE_IMPORT as u16)
        );
        assert_eq!(
            store
                .migration_marker(community, &owner, &owner)
                .await
                .expect("owner migration marker"),
            Some(SectionWorkspaceMigrationMarker {
                source_event_id: vec![0x11; 32],
                source_hash: vec![0x22; 32],
            })
        );
    }

    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn failed_import_rolls_back_and_different_second_import_is_rejected() {
        let pool = setup_pool().await;
        let community = make_community(&pool).await;
        let owner_keys = Keys::generate();
        let owner = owner_keys.public_key().to_bytes();
        let store = SectionWorkspaceStore::new(pool.clone());
        let missing_channel = Uuid::new_v4();
        let mut import = import_fixture(Uuid::new_v4(), missing_channel, Uuid::new_v4());
        let signed_import = import_event(&owner_keys, &owner, &import);

        assert!(matches!(
            store
                .import_v1(
                    community,
                    &owner,
                    &owner,
                    &signed_import.0,
                    &signed_import.1,
                    &import
                )
                .await,
            Err(DbError::InvalidData(_))
        ));
        let rows: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM section_workspaces WHERE community_id = $1 AND owner_pubkey = $2",
        )
        .bind(community.as_uuid())
        .bind(owner)
        .fetch_one(&pool)
        .await
        .expect("count rolled-back workspace");
        assert_eq!(rows, 0, "failed import must leave no partial workspace");

        make_channel(&pool, community, missing_channel).await;
        let signed_import = import_event(&owner_keys, &owner, &import);
        store
            .import_v1(
                community,
                &owner,
                &owner,
                &signed_import.0,
                &signed_import.1,
                &import,
            )
            .await
            .expect("valid import after rollback");
        import.action_id = Uuid::new_v4();
        let second_import_event = import_event(&owner_keys, &owner, &import);
        assert!(matches!(
            store
                .import_v1(community, &owner, &owner, &second_import_event.0, &second_import_event.1, &import)
                .await,
            Err(DbError::InvalidData(message)) if message == "workspace has already been migrated"
        ));
    }

    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn workspace_access_and_import_are_confined_to_community() {
        let pool = setup_pool().await;
        let community_a = make_community(&pool).await;
        let community_b = make_community(&pool).await;
        let owner_keys = Keys::generate();
        let owner = owner_keys.public_key().to_bytes();
        let viewer = [8_u8; 32];
        let shared_channel_id = Uuid::new_v4();
        let shared_section_id = Uuid::new_v4();
        make_channel(&pool, community_a, shared_channel_id).await;
        make_channel(&pool, community_b, shared_channel_id).await;
        let store = SectionWorkspaceStore::new(pool);
        let import = import_fixture(Uuid::new_v4(), shared_channel_id, shared_section_id);
        let signed_import = import_event(&owner_keys, &owner, &import);
        store
            .import_v1(
                community_a,
                &owner,
                &owner,
                &signed_import.0,
                &signed_import.1,
                &import,
            )
            .await
            .expect("community A import");
        let grant_action = Uuid::new_v4();
        let signed_grant_1 = grant_event(
            &owner_keys,
            &owner,
            &viewer,
            SectionWorkspaceRole::Viewer,
            1,
            "viewer-envelope-epoch-1",
            grant_action,
        );
        store
            .grant(
                community_a,
                &owner,
                &owner,
                &viewer,
                SectionWorkspaceRole::Viewer,
                1,
                "viewer-envelope-epoch-1",
                grant_action,
                &signed_grant_1.0,
                &signed_grant_1.1,
            )
            .await
            .expect("community A viewer grant");

        assert!(store
            .projection(community_a, &owner, &viewer)
            .await
            .expect("community A projection")
            .is_some());
        assert!(store
            .projection(community_b, &owner, &viewer)
            .await
            .expect("community B projection")
            .is_none());

        let import_b = import_fixture(Uuid::new_v4(), shared_channel_id, shared_section_id);
        let import_b_event = import_event(&owner_keys, &owner, &import_b);
        assert_eq!(
            store
                .import_v1(
                    community_b,
                    &owner,
                    &owner,
                    &import_b_event.0,
                    &import_b_event.1,
                    &import_b
                )
                .await
                .expect("community B import"),
            ImportOutcome::Imported { revision: 1 }
        );
    }

    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn grant_epoch_and_revoke_rotation_are_atomic() {
        let pool = setup_pool().await;
        let community = make_community(&pool).await;
        let owner_keys = Keys::generate();
        let owner = owner_keys.public_key().to_bytes();
        let revoked = [13_u8; 32];
        let remaining = [14_u8; 32];
        let channel_id = Uuid::new_v4();
        let section_id = Uuid::new_v4();
        make_channel(&pool, community, channel_id).await;
        let store = SectionWorkspaceStore::new(pool);
        let import = import_fixture(Uuid::new_v4(), channel_id, section_id);
        let signed_import = import_event(&owner_keys, &owner, &import);
        store
            .import_v1(
                community,
                &owner,
                &owner,
                &signed_import.0,
                &signed_import.1,
                &import,
            )
            .await
            .expect("import");

        let wrong_action = Uuid::new_v4();
        let signed_grant_2 = grant_event(
            &owner_keys,
            &owner,
            &revoked,
            SectionWorkspaceRole::Viewer,
            2,
            "wrong-epoch-envelope",
            wrong_action,
        );
        assert!(matches!(
            store
                .grant(
                    community,
                    &owner,
                    &owner,
                    &revoked,
                    SectionWorkspaceRole::Viewer,
                    2,
                    "wrong-epoch-envelope",
                    wrong_action,
                    &signed_grant_2.0,
                    &signed_grant_2.1,
                )
                .await,
            Err(DbError::InvalidData(_))
        ));
        for reader in [&revoked, &remaining] {
            let grant_action = Uuid::new_v4();
            let signed_grant_3 = grant_event(
                &owner_keys,
                &owner,
                reader,
                SectionWorkspaceRole::Viewer,
                1,
                "viewer-envelope-epoch-1",
                grant_action,
            );
            store
                .grant(
                    community,
                    &owner,
                    &owner,
                    reader,
                    SectionWorkspaceRole::Viewer,
                    1,
                    "viewer-envelope-epoch-1",
                    grant_action,
                    &signed_grant_3.0,
                    &signed_grant_3.1,
                )
                .await
                .expect("viewer grant");
        }

        let action_id = Uuid::new_v4();
        let sections = [RotatedSectionMetadata {
            section_id,
            encrypted_label: "label-epoch-2".into(),
            encrypted_icon: None,
        }];
        let envelopes = [
            KeyEnvelopeUpdate {
                reader_pubkey: owner.to_vec(),
                envelope: "owner-envelope-epoch-2".into(),
            },
            KeyEnvelopeUpdate {
                reader_pubkey: remaining.to_vec(),
                envelope: "remaining-envelope-epoch-2".into(),
            },
        ];
        let signed_revoke_1 = revoke_event(
            &owner_keys,
            &owner,
            &revoked,
            2,
            &sections,
            &envelopes,
            action_id,
        );
        assert_eq!(
            store
                .revoke_and_rotate(
                    community,
                    &owner,
                    &owner,
                    &revoked,
                    2,
                    &sections,
                    &envelopes,
                    action_id,
                    &signed_revoke_1.0,
                    &signed_revoke_1.1,
                )
                .await
                .expect("revoke and rotate"),
            GrantMutationOutcome::Applied { revision: 4 }
        );
        assert!(store
            .projection(community, &owner, &revoked)
            .await
            .expect("revoked projection")
            .is_none());
        let remaining_projection = store
            .projection(community, &owner, &remaining)
            .await
            .expect("remaining projection")
            .expect("remaining reader stays authorized");
        assert_eq!(remaining_projection.revision, 4);
        assert_eq!(remaining_projection.key_epoch, 2);
        assert_eq!(
            remaining_projection.sections[0].encrypted_label,
            "label-epoch-2"
        );
        assert_eq!(
            remaining_projection.reader_key_envelope,
            "remaining-envelope-epoch-2"
        );
        let signed_revoke_2 = revoke_event(
            &owner_keys,
            &owner,
            &revoked,
            2,
            &sections,
            &envelopes,
            action_id,
        );
        assert_eq!(
            store
                .revoke_and_rotate(
                    community,
                    &owner,
                    &owner,
                    &revoked,
                    2,
                    &sections,
                    &envelopes,
                    action_id,
                    &signed_revoke_2.0,
                    &signed_revoke_2.1,
                )
                .await
                .expect("exact revoke retry"),
            GrantMutationOutcome::AlreadyApplied { revision: 4 }
        );
    }
}
