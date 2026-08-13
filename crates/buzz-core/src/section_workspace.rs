//! Owner-scoped channel-section workspace protocol types.
//!
//! The relay is authoritative for structure, assignments, grants, revisions,
//! and command ordering. Human-readable labels and icons remain opaque
//! ciphertext encrypted by clients with a per-workspace content key.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use thiserror::Error;
use uuid::Uuid;

/// Maximum active sections in one workspace.
pub const MAX_SECTIONS: usize = 100;
/// Maximum channel assignments in one workspace.
pub const MAX_ASSIGNMENTS: usize = 1_000;
/// Maximum active delegate grants in one workspace.
pub const MAX_GRANTS: usize = 256;
/// Maximum bytes in one encrypted label or icon field.
pub const MAX_ENCRYPTED_METADATA_BYTES: usize = 65_535;
/// Maximum bytes in one pairwise-wrapped content-key envelope.
pub const MAX_KEY_ENVELOPE_BYTES: usize = 4_096;
/// Wire schema version implemented by this protocol.
pub const SECTION_WORKSPACE_VERSION: u32 = 1;

/// Delegated authority over an owner's section workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SectionWorkspaceRole {
    /// Read and decrypt the current projection.
    Viewer,
    /// Viewer rights plus assign and unassign channels.
    Mover,
    /// Mover rights plus section create, rename, delete, and reorder.
    Manager,
}

impl SectionWorkspaceRole {
    /// Stable database and wire spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Viewer => "viewer",
            Self::Mover => "mover",
            Self::Manager => "manager",
        }
    }
}

/// An encrypted section supplied during revision-zero migration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SectionWorkspaceImportSection {
    /// Existing client section UUID, preserved across migration.
    pub id: Uuid,
    /// Zero-based display order.
    pub rank: u32,
    /// Authenticated ciphertext for the section label.
    pub encrypted_label: String,
    /// Optional authenticated ciphertext for the section icon.
    pub encrypted_icon: Option<String>,
}

/// A channel-to-section assignment supplied during migration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SectionWorkspaceImportAssignment {
    /// Channel UUID in the same server-resolved community.
    pub channel_id: Uuid,
    /// Destination section UUID in this import.
    pub section_id: Uuid,
}

/// Revision-zero import command body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SectionWorkspaceImport {
    /// Wire schema version.
    pub version: u32,
    /// Stable retry identity independent of the signed transport event id.
    pub action_id: Uuid,
    /// Legacy kind-30078 event id whose plaintext was migrated.
    pub source_event_id: String,
    /// Lowercase SHA-256 hex of the canonical legacy plaintext.
    pub source_hash: String,
    /// Initial content-key epoch. Version 1 imports require epoch 1.
    pub key_epoch: u64,
    /// Owner's pairwise-wrapped content-key envelope.
    pub owner_key_envelope: String,
    /// Ordered encrypted sections.
    pub sections: Vec<SectionWorkspaceImportSection>,
    /// Channel assignments.
    pub assignments: Vec<SectionWorkspaceImportAssignment>,
}

/// Wire projection migration marker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SectionWorkspaceProjectionMigration {
    /// Imported legacy event ID.
    pub source_event_id: String,
    /// Canonical legacy plaintext hash.
    pub source_hash: String,
}

/// One section in the relay's reader-specific projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SectionWorkspaceProjectedSection {
    /// Stable section identifier.
    pub id: Uuid,
    /// Zero-based display order.
    pub rank: u32,
    /// Authenticated label ciphertext.
    pub encrypted_label: String,
    /// Optional authenticated icon ciphertext.
    pub encrypted_icon: Option<String>,
}

/// One assignment in the relay's reader-specific projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SectionWorkspaceProjectedAssignment {
    /// Assigned channel identifier.
    pub channel_id: Uuid,
    /// Destination section identifier.
    pub section_id: Uuid,
    /// Per-assignment revision.
    pub revision: u64,
}

/// Complete reader-specific current projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SectionWorkspaceProjection {
    /// Wire schema version.
    pub version: u32,
    /// Workspace owner as lowercase hex.
    pub owner_pubkey: String,
    /// Monotonic workspace revision.
    pub revision: u64,
    /// Monotonic layout revision.
    pub layout_revision: u64,
    /// Current content-key epoch.
    pub key_epoch: u64,
    /// Imported legacy source marker.
    pub migration: SectionWorkspaceProjectionMigration,
    /// Pairwise envelope for this projection reader.
    pub reader_key_envelope: String,
    /// Ordered active sections.
    pub sections: Vec<SectionWorkspaceProjectedSection>,
    /// Current channel assignments.
    pub assignments: Vec<SectionWorkspaceProjectedAssignment>,
}

/// Client action for an incoming projection revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectionRevisionAction {
    /// Persist and render the projection.
    Accept,
    /// Discard the delta and fetch the current full projection.
    Refetch,
    /// Discard a stale or duplicate projection.
    Ignore,
}

/// Apply the frozen projection revision rule.
pub fn projection_revision_action(
    previous_revision: Option<u64>,
    incoming_revision: u64,
) -> ProjectionRevisionAction {
    match previous_revision {
        None => ProjectionRevisionAction::Accept,
        Some(previous) if incoming_revision <= previous => ProjectionRevisionAction::Ignore,
        Some(previous) if incoming_revision == previous.saturating_add(1) => {
            ProjectionRevisionAction::Accept
        }
        Some(_) => ProjectionRevisionAction::Refetch,
    }
}

/// Owner-only grant command body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SectionWorkspaceGrantCommand {
    /// Wire schema version.
    pub version: u32,
    /// Stable retry identity.
    pub action_id: Uuid,
    /// Exact delegate pubkey as lowercase hex.
    pub actor_pubkey: String,
    /// Granted authority.
    pub role: SectionWorkspaceRole,
    /// Current workspace key epoch.
    pub key_epoch: u64,
    /// Pairwise NIP-44 wrapped content key.
    pub key_envelope: String,
}

/// One metadata replacement in a revoke-and-rotate command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SectionWorkspaceRotatedSection {
    /// Existing section UUID.
    pub section_id: Uuid,
    /// Metadata envelope encrypted under the new workspace key.
    pub encrypted_label: String,
    /// Optional icon envelope encrypted under the new workspace key.
    pub encrypted_icon: Option<String>,
}

/// One reader envelope in a revoke-and-rotate command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SectionWorkspaceReaderEnvelope {
    /// Exact reader pubkey as lowercase hex.
    pub reader_pubkey: String,
    /// Pairwise NIP-44 wrapped content key.
    pub key_envelope: String,
}

/// Owner-only revoke-and-rotate command body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SectionWorkspaceRevokeCommand {
    /// Wire schema version.
    pub version: u32,
    /// Stable retry identity.
    pub action_id: Uuid,
    /// Revoked delegate pubkey as lowercase hex.
    pub actor_pubkey: String,
    /// Exactly the current epoch plus one.
    pub new_key_epoch: u64,
    /// Every active section, re-encrypted exactly once.
    pub sections: Vec<SectionWorkspaceRotatedSection>,
    /// Owner and every remaining reader, exactly once.
    pub envelopes: Vec<SectionWorkspaceReaderEnvelope>,
}

/// Produce the byte-exact NIP-SW canonical JSON profile.
///
/// Objects are recursively key-sorted, arrays retain order, no insignificant
/// whitespace is emitted, and only integer JSON numbers are accepted. Strings
/// use serde_json's UTF-8/escape encoding. Duplicate keys must be rejected by
/// the caller's strict typed deserializer before canonicalization.
pub fn canonical_json(
    value: &serde_json::Value,
) -> Result<String, SectionWorkspaceValidationError> {
    fn write(
        value: &serde_json::Value,
        out: &mut String,
    ) -> Result<(), SectionWorkspaceValidationError> {
        match value {
            serde_json::Value::Null => out.push_str("null"),
            serde_json::Value::Bool(v) => out.push_str(if *v { "true" } else { "false" }),
            serde_json::Value::Number(v) if v.is_i64() || v.is_u64() => {
                out.push_str(&v.to_string())
            }
            serde_json::Value::Number(_) => {
                return Err(SectionWorkspaceValidationError::Invalid(
                    "non_integer_number",
                ))
            }
            serde_json::Value::String(v) => out.push_str(
                &serde_json::to_string(v)
                    .map_err(|_| SectionWorkspaceValidationError::Invalid("json_string"))?,
            ),
            serde_json::Value::Array(values) => {
                out.push('[');
                for (index, value) in values.iter().enumerate() {
                    if index != 0 {
                        out.push(',');
                    }
                    write(value, out)?;
                }
                out.push(']');
            }
            serde_json::Value::Object(values) => {
                out.push('{');
                let mut keys = values.keys().collect::<Vec<_>>();
                keys.sort_unstable();
                for (index, key) in keys.into_iter().enumerate() {
                    if index != 0 {
                        out.push(',');
                    }
                    out.push_str(
                        &serde_json::to_string(key)
                            .map_err(|_| SectionWorkspaceValidationError::Invalid("json_key"))?,
                    );
                    out.push(':');
                    write(&values[key], out)?;
                }
                out.push('}');
            }
        }
        Ok(())
    }
    let mut out = String::new();
    write(value, &mut out)?;
    Ok(out)
}

/// Validate the exact routing-tag grammar shared by command handlers.
pub fn validate_command_tags(
    tags: &[Vec<String>],
    owner_pubkey: &str,
    actor_pubkey: Option<&str>,
    action_id: Uuid,
) -> Result<(), SectionWorkspaceValidationError> {
    let mut owner = None;
    let mut actor = None;
    let mut action = None;
    for tag in tags {
        if tag.len() != 2 {
            return Err(SectionWorkspaceValidationError::Invalid("tag_grammar"));
        }
        let slot = match tag[0].as_str() {
            "p" => &mut owner,
            "actor" if actor_pubkey.is_some() => &mut actor,
            "action" => &mut action,
            _ => return Err(SectionWorkspaceValidationError::Invalid("tag_grammar")),
        };
        if slot.replace(tag[1].as_str()).is_some() {
            return Err(SectionWorkspaceValidationError::Invalid("tag_grammar"));
        }
    }
    let expected_action = action_id.to_string();
    if owner != Some(owner_pubkey)
        || actor != actor_pubkey
        || action != Some(expected_action.as_str())
    {
        return Err(SectionWorkspaceValidationError::Invalid("tag_grammar"));
    }
    Ok(())
}

/// Protocol validation failures independent of any transport or database.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SectionWorkspaceValidationError {
    /// A collection exceeds its wire bound.
    #[error("{field} exceeds maximum {maximum}")]
    TooMany {
        /// Field name.
        field: &'static str,
        /// Maximum accepted count.
        maximum: usize,
    },
    /// A required identifier is nil, malformed, or duplicated.
    #[error("invalid {0}")]
    Invalid(&'static str),
    /// An encrypted field is empty or exceeds its byte bound.
    #[error("invalid encrypted {0}")]
    InvalidCiphertext(&'static str),
}

impl SectionWorkspaceGrantCommand {
    /// Validate semantic fields after strict decoding.
    pub fn validate(&self) -> Result<(), SectionWorkspaceValidationError> {
        if self.version != SECTION_WORKSPACE_VERSION {
            return Err(SectionWorkspaceValidationError::Invalid("version"));
        }
        if self.action_id.is_nil() {
            return Err(SectionWorkspaceValidationError::Invalid("action_id"));
        }
        validate_hex_32(&self.actor_pubkey, "actor_pubkey")?;
        if self.key_epoch == 0 {
            return Err(SectionWorkspaceValidationError::Invalid("key_epoch"));
        }
        validate_ciphertext(&self.key_envelope, MAX_KEY_ENVELOPE_BYTES, "key_envelope")
    }
}

impl SectionWorkspaceRevokeCommand {
    /// Validate semantic fields after strict decoding.
    pub fn validate(&self) -> Result<(), SectionWorkspaceValidationError> {
        if self.version != SECTION_WORKSPACE_VERSION {
            return Err(SectionWorkspaceValidationError::Invalid("version"));
        }
        if self.action_id.is_nil() {
            return Err(SectionWorkspaceValidationError::Invalid("action_id"));
        }
        validate_hex_32(&self.actor_pubkey, "actor_pubkey")?;
        if self.new_key_epoch == 0 {
            return Err(SectionWorkspaceValidationError::Invalid("new_key_epoch"));
        }
        for section in &self.sections {
            if section.section_id.is_nil() {
                return Err(SectionWorkspaceValidationError::Invalid("section_id"));
            }
            validate_ciphertext(
                &section.encrypted_label,
                MAX_ENCRYPTED_METADATA_BYTES,
                "label",
            )?;
            if let Some(icon) = &section.encrypted_icon {
                validate_ciphertext(icon, MAX_ENCRYPTED_METADATA_BYTES, "icon")?;
            }
        }
        for envelope in &self.envelopes {
            validate_hex_32(&envelope.reader_pubkey, "reader_pubkey")?;
            validate_ciphertext(
                &envelope.key_envelope,
                MAX_KEY_ENVELOPE_BYTES,
                "key_envelope",
            )?;
        }
        Ok(())
    }
}

impl SectionWorkspaceProjection {
    /// Validate the wire version before a projection is accepted.
    pub fn validate(&self) -> Result<(), SectionWorkspaceValidationError> {
        if self.version != SECTION_WORKSPACE_VERSION {
            return Err(SectionWorkspaceValidationError::Invalid("version"));
        }
        Ok(())
    }
}

impl SectionWorkspaceImport {
    /// Validate bounds and referential shape before persistence.
    pub fn validate(&self) -> Result<(), SectionWorkspaceValidationError> {
        if self.version != SECTION_WORKSPACE_VERSION {
            return Err(SectionWorkspaceValidationError::Invalid("version"));
        }
        if self.sections.len() > MAX_SECTIONS {
            return Err(SectionWorkspaceValidationError::TooMany {
                field: "sections",
                maximum: MAX_SECTIONS,
            });
        }
        if self.assignments.len() > MAX_ASSIGNMENTS {
            return Err(SectionWorkspaceValidationError::TooMany {
                field: "assignments",
                maximum: MAX_ASSIGNMENTS,
            });
        }
        if self.action_id.is_nil() {
            return Err(SectionWorkspaceValidationError::Invalid("action_id"));
        }
        if self.key_epoch != 1 {
            return Err(SectionWorkspaceValidationError::Invalid("key_epoch"));
        }
        validate_hex_32(&self.source_event_id, "source_event_id")?;
        validate_hex_32(&self.source_hash, "source_hash")?;
        validate_ciphertext(
            &self.owner_key_envelope,
            MAX_KEY_ENVELOPE_BYTES,
            "owner_key_envelope",
        )?;

        let mut ids = HashSet::with_capacity(self.sections.len());
        let mut ranks = HashSet::with_capacity(self.sections.len());
        for section in &self.sections {
            if section.id.is_nil() || !ids.insert(section.id) {
                return Err(SectionWorkspaceValidationError::Invalid("section_id"));
            }
            if section.rank as usize >= self.sections.len() || !ranks.insert(section.rank) {
                return Err(SectionWorkspaceValidationError::Invalid("section_rank"));
            }
            validate_ciphertext(
                &section.encrypted_label,
                MAX_ENCRYPTED_METADATA_BYTES,
                "label",
            )?;
            if let Some(icon) = &section.encrypted_icon {
                validate_ciphertext(icon, MAX_ENCRYPTED_METADATA_BYTES, "icon")?;
            }
        }
        let mut channels = HashSet::with_capacity(self.assignments.len());
        for assignment in &self.assignments {
            if assignment.channel_id.is_nil()
                || !channels.insert(assignment.channel_id)
                || !ids.contains(&assignment.section_id)
            {
                return Err(SectionWorkspaceValidationError::Invalid("assignment"));
            }
        }
        Ok(())
    }
}

fn validate_hex_32(
    value: &str,
    field: &'static str,
) -> Result<(), SectionWorkspaceValidationError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(SectionWorkspaceValidationError::Invalid(field));
    }
    Ok(())
}

fn validate_ciphertext(
    value: &str,
    maximum: usize,
    field: &'static str,
) -> Result<(), SectionWorkspaceValidationError> {
    if value.is_empty() || value.len() > maximum {
        return Err(SectionWorkspaceValidationError::InvalidCiphertext(field));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_import() -> SectionWorkspaceImport {
        let section_id = Uuid::new_v4();
        SectionWorkspaceImport {
            version: SECTION_WORKSPACE_VERSION,
            action_id: Uuid::new_v4(),
            source_event_id: "11".repeat(32),
            source_hash: "22".repeat(32),
            key_epoch: 1,
            owner_key_envelope: "wrapped-key".into(),
            sections: vec![SectionWorkspaceImportSection {
                id: section_id,
                rank: 0,
                encrypted_label: "ciphertext".into(),
                encrypted_icon: None,
            }],
            assignments: vec![SectionWorkspaceImportAssignment {
                channel_id: Uuid::new_v4(),
                section_id,
            }],
        }
    }

    #[test]
    fn valid_revision_zero_import_passes() {
        valid_import().validate().expect("valid import");
    }

    #[test]
    fn duplicate_channel_assignment_is_rejected() {
        let mut import = valid_import();
        import.assignments.push(import.assignments[0]);
        assert_eq!(
            import.validate(),
            Err(SectionWorkspaceValidationError::Invalid("assignment"))
        );
    }

    #[test]
    fn ranks_must_be_a_complete_permutation() {
        let mut import = valid_import();
        import.sections.push(SectionWorkspaceImportSection {
            id: Uuid::new_v4(),
            rank: 0,
            encrypted_label: "ciphertext".into(),
            encrypted_icon: None,
        });
        assert_eq!(
            import.validate(),
            Err(SectionWorkspaceValidationError::Invalid("section_rank"))
        );
    }
}

#[cfg(test)]
mod fixture_tests {
    use super::*;
    use serde_json::Value;

    fn fixture() -> Value {
        serde_json::from_str(include_str!("../../../docs/nips/NIP-SW.fixtures.json"))
            .expect("valid NIP-SW fixture JSON")
    }

    #[test]
    fn shared_fixture_matches_protocol_constants_and_role_matrix() {
        let fixture = fixture();
        assert_eq!(fixture["version"], SECTION_WORKSPACE_VERSION);
        for (name, kind) in [
            ("import_v1", crate::kind::KIND_SECTION_WORKSPACE_IMPORT),
            ("grant", crate::kind::KIND_SECTION_WORKSPACE_GRANT),
            ("revoke", crate::kind::KIND_SECTION_WORKSPACE_REVOKE),
            ("move", crate::kind::KIND_SECTION_WORKSPACE_MOVE),
            ("manage", crate::kind::KIND_SECTION_WORKSPACE_MANAGE),
            ("projection", crate::kind::KIND_SECTION_WORKSPACE_PROJECTION),
        ] {
            assert_eq!(fixture["kinds"][name], kind, "kind {name}");
        }
        assert_eq!(fixture["limits"]["sections"], MAX_SECTIONS);
        assert_eq!(fixture["limits"]["assignments"], MAX_ASSIGNMENTS);
        assert_eq!(fixture["limits"]["grants"], MAX_GRANTS);
        assert_eq!(
            fixture["limits"]["encrypted_metadata_bytes"],
            MAX_ENCRYPTED_METADATA_BYTES
        );
        assert_eq!(
            fixture["limits"]["key_envelope_bytes"],
            MAX_KEY_ENVELOPE_BYTES
        );
        assert_eq!(
            fixture["roles"],
            serde_json::json!(["viewer", "mover", "manager"])
        );
        assert_eq!(
            fixture["role_matrix"],
            serde_json::json!({
                "owner": ["read", "grant", "revoke", "move", "manage", "import_v1"],
                "viewer": ["read"],
                "mover": ["read", "move"],
                "manager": ["read", "move", "manage"]
            })
        );
        assert_eq!(fixture["metadata_crypto"]["algorithm"], "AES-256-GCM");
    }

    #[test]
    fn projection_vectors_are_typed_and_drive_revision_behavior() {
        let fixture = fixture();
        let cases = fixture["projection_cases"]
            .as_array()
            .expect("projection cases");
        let accepted =
            serde_json::from_value::<SectionWorkspaceProjection>(cases[0]["projection"].clone())
                .expect("complete projection shape");
        accepted.validate().expect("supported projection version");
        assert_eq!(accepted.sections.len(), 2);
        assert_eq!(accepted.assignments.len(), 1);

        for case in cases {
            let incoming = case["projection"]["revision"].as_u64().expect("revision");
            let previous = case.get("previous_revision").and_then(Value::as_u64);
            let actual = projection_revision_action(previous, incoming);
            let expected = match case["expect"].as_str().expect("expect") {
                "accept" => ProjectionRevisionAction::Accept,
                "refetch" => ProjectionRevisionAction::Refetch,
                "ignore" => ProjectionRevisionAction::Ignore,
                other => panic!("unknown projection expectation {other}"),
            };
            assert_eq!(actual, expected, "projection case {}", case["name"]);
        }

        let mut unknown = cases[0]["projection"].clone();
        unknown["extra"] = serde_json::json!(true);
        assert!(serde_json::from_value::<SectionWorkspaceProjection>(unknown).is_err());
        let mut wrong_version = cases[0]["projection"].clone();
        wrong_version["version"] = serde_json::json!(2);
        assert_eq!(
            serde_json::from_value::<SectionWorkspaceProjection>(wrong_version)
                .expect("typed wrong version")
                .validate(),
            Err(SectionWorkspaceValidationError::Invalid("version"))
        );
    }

    #[test]
    fn canonicalization_and_every_typed_command_vector_match() {
        use sha2::{Digest, Sha256};
        let fixture = fixture();
        let legacy = &fixture["canonicalization"]["legacy_plaintext"];
        let canonical = canonical_json(&legacy["input"]).expect("canonical legacy plaintext");
        assert_eq!(canonical, legacy["canonical"]);
        assert_eq!(
            hex::encode(Sha256::digest(canonical.as_bytes())),
            legacy["sha256"]
        );

        let imported: SectionWorkspaceImport = serde_json::from_str(
            fixture["canonicalization"]["import_command"]["canonical"]
                .as_str()
                .expect("canonical import"),
        )
        .expect("typed canonical import");
        imported.validate().expect("valid import vector");
        let import_value = serde_json::to_value(&imported).expect("import value");
        let import_canonical = canonical_json(&import_value).expect("canonical typed import");
        assert_eq!(
            import_canonical,
            fixture["canonicalization"]["import_command"]["canonical"]
        );
        assert_eq!(
            hex::encode(Sha256::digest(import_canonical.as_bytes())),
            fixture["canonicalization"]["import_command"]["sha256"]
        );

        for case in fixture["command_cases"].as_array().expect("command cases") {
            let Some(command) = case.get("command") else {
                continue;
            };
            let parsed =
                if case["name"].as_str().unwrap().starts_with("grant-") {
                    serde_json::from_value::<SectionWorkspaceGrantCommand>(command.clone())
                        .and_then(|value| {
                            value.validate().map_err(serde::de::Error::custom)?;
                            Ok(())
                        })
                } else {
                    serde_json::from_value::<SectionWorkspaceRevokeCommand>(command.clone())
                        .and_then(|value| {
                            value.validate().map_err(serde::de::Error::custom)?;
                            Ok(())
                        })
                };
            assert_eq!(
                parsed.is_ok(),
                case["expect"] == "accept",
                "command case {}",
                case["name"]
            );
        }
        for mut command in [
            serde_json::to_value(&imported).unwrap(),
            fixture["command_cases"][0]["command"].clone(),
            fixture["command_cases"][2]["command"].clone(),
        ] {
            command["version"] = serde_json::json!(2);
            let valid = if command.get("source_event_id").is_some() {
                serde_json::from_value::<SectionWorkspaceImport>(command).and_then(|value| {
                    value.validate().map_err(serde::de::Error::custom)?;
                    Ok(())
                })
            } else if command.get("role").is_some() {
                serde_json::from_value::<SectionWorkspaceGrantCommand>(command).and_then(|value| {
                    value.validate().map_err(serde::de::Error::custom)?;
                    Ok(())
                })
            } else {
                serde_json::from_value::<SectionWorkspaceRevokeCommand>(command).and_then(|value| {
                    value.validate().map_err(serde::de::Error::custom)?;
                    Ok(())
                })
            };
            assert!(valid.is_err(), "wrong protocol version must fail");
        }
    }

    #[test]
    fn command_tag_grammar_rejects_missing_duplicate_unknown_and_mismatched_tags() {
        let action_id = Uuid::parse_str("eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee").unwrap();
        let owner = "11".repeat(32);
        let actor = "44".repeat(32);
        let valid = vec![
            vec!["p".into(), owner.clone()],
            vec!["actor".into(), actor.clone()],
            vec!["action".into(), action_id.to_string()],
        ];
        assert!(validate_command_tags(&valid, &owner, Some(&actor), action_id).is_ok());
        for invalid in [
            valid[..2].to_vec(),
            [valid.clone(), vec![valid[0].clone()]].concat(),
            [valid.clone(), vec![vec!["extra".into(), "x".into()]]].concat(),
            vec![
                valid[0].clone(),
                valid[1].clone(),
                vec!["action".into(), Uuid::nil().to_string()],
            ],
        ] {
            assert_eq!(
                validate_command_tags(&invalid, &owner, Some(&actor), action_id),
                Err(SectionWorkspaceValidationError::Invalid("tag_grammar"))
            );
        }
    }

    #[test]
    fn strict_command_decoding_rejects_duplicates_and_malformed_values() {
        let duplicate = r#"{"version":1,"version":1,"action_id":"eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee","actor_pubkey":"4444444444444444444444444444444444444444444444444444444444444444","role":"viewer","key_epoch":1,"key_envelope":"wrapped"}"#;
        assert!(serde_json::from_str::<SectionWorkspaceGrantCommand>(duplicate).is_err());
        let mut malformed = fixture()["command_cases"][0]["command"].clone();
        malformed["role"] = serde_json::json!("owner");
        assert!(serde_json::from_value::<SectionWorkspaceGrantCommand>(malformed).is_err());
        assert!(canonical_json(&serde_json::json!({"fraction": 1.5})).is_err());
    }
}
