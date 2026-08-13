-- Relay-authoritative owner-scoped channel-section workspaces.
SET LOCAL lock_timeout = '5s';

CREATE TABLE section_workspaces (
    community_id UUID NOT NULL REFERENCES communities(id),
    owner_pubkey BYTEA NOT NULL CHECK (length(owner_pubkey) = 32),
    revision BIGINT NOT NULL DEFAULT 0 CHECK (revision >= 0),
    layout_revision BIGINT NOT NULL DEFAULT 0 CHECK (layout_revision >= 0),
    key_epoch BIGINT NOT NULL DEFAULT 0 CHECK (key_epoch >= 0),
    migrated_at TIMESTAMPTZ,
    migration_source_event_id BYTEA CHECK (migration_source_event_id IS NULL OR length(migration_source_event_id) = 32),
    migration_source_hash BYTEA CHECK (migration_source_hash IS NULL OR length(migration_source_hash) = 32),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (community_id, owner_pubkey),
    CHECK ((migrated_at IS NULL) = (migration_source_event_id IS NULL)),
    CHECK ((migrated_at IS NULL) = (migration_source_hash IS NULL)),
    CHECK ((migrated_at IS NULL) = (revision = 0))
);

CREATE TABLE section_grants (
    community_id UUID NOT NULL,
    owner_pubkey BYTEA NOT NULL CHECK (length(owner_pubkey) = 32),
    actor_pubkey BYTEA NOT NULL CHECK (length(actor_pubkey) = 32),
    role TEXT NOT NULL CHECK (role IN ('viewer', 'mover', 'manager')),
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'revoked')),
    granted_by BYTEA NOT NULL CHECK (length(granted_by) = 32),
    granted_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_at TIMESTAMPTZ,
    PRIMARY KEY (community_id, owner_pubkey, actor_pubkey),
    FOREIGN KEY (community_id, owner_pubkey)
        REFERENCES section_workspaces(community_id, owner_pubkey) ON DELETE CASCADE,
    CHECK (actor_pubkey <> owner_pubkey),
    CHECK ((status = 'revoked') = (revoked_at IS NOT NULL))
);
CREATE INDEX section_grants_actor_active
    ON section_grants (community_id, actor_pubkey, owner_pubkey)
    WHERE status = 'active';

CREATE TABLE section_key_envelopes (
    community_id UUID NOT NULL,
    owner_pubkey BYTEA NOT NULL CHECK (length(owner_pubkey) = 32),
    reader_pubkey BYTEA NOT NULL CHECK (length(reader_pubkey) = 32),
    key_epoch BIGINT NOT NULL CHECK (key_epoch > 0),
    envelope TEXT NOT NULL CHECK (octet_length(envelope) BETWEEN 1 AND 4096),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (community_id, owner_pubkey, reader_pubkey, key_epoch),
    FOREIGN KEY (community_id, owner_pubkey)
        REFERENCES section_workspaces(community_id, owner_pubkey) ON DELETE CASCADE
);

CREATE TABLE sections (
    community_id UUID NOT NULL,
    owner_pubkey BYTEA NOT NULL CHECK (length(owner_pubkey) = 32),
    section_id UUID NOT NULL CHECK (section_id <> '00000000-0000-0000-0000-000000000000'::uuid),
    encrypted_label TEXT NOT NULL CHECK (octet_length(encrypted_label) BETWEEN 1 AND 65535),
    encrypted_icon TEXT CHECK (encrypted_icon IS NULL OR octet_length(encrypted_icon) BETWEEN 1 AND 65535),
    rank INTEGER NOT NULL CHECK (rank BETWEEN 0 AND 99),
    revision BIGINT NOT NULL CHECK (revision > 0),
    deleted_at TIMESTAMPTZ,
    PRIMARY KEY (community_id, owner_pubkey, section_id),
    FOREIGN KEY (community_id, owner_pubkey)
        REFERENCES section_workspaces(community_id, owner_pubkey) ON DELETE CASCADE
);
CREATE UNIQUE INDEX sections_active_rank
    ON sections (community_id, owner_pubkey, rank) WHERE deleted_at IS NULL;

CREATE TABLE section_assignments (
    community_id UUID NOT NULL,
    owner_pubkey BYTEA NOT NULL CHECK (length(owner_pubkey) = 32),
    channel_id UUID NOT NULL,
    section_id UUID NOT NULL,
    revision BIGINT NOT NULL CHECK (revision > 0),
    updated_by BYTEA NOT NULL CHECK (length(updated_by) = 32),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (community_id, owner_pubkey, channel_id),
    FOREIGN KEY (community_id, owner_pubkey, section_id)
        REFERENCES sections(community_id, owner_pubkey, section_id) ON DELETE CASCADE,
    FOREIGN KEY (community_id, channel_id)
        REFERENCES channels(community_id, id) ON DELETE CASCADE
);

CREATE TABLE section_actions (
    community_id UUID NOT NULL,
    owner_pubkey BYTEA NOT NULL CHECK (length(owner_pubkey) = 32),
    action_id UUID NOT NULL,
    actor_pubkey BYTEA NOT NULL CHECK (length(actor_pubkey) = 32),
    command_kind TEXT NOT NULL CHECK (command_kind IN (
        'import_v1', 'grant', 'revoke', 'assign_channel', 'unassign_channel',
        'create_section', 'rename_section', 'delete_section', 'reorder_sections'
    )),
    command_event_id BYTEA NOT NULL CHECK (length(command_event_id) = 32),
    signed_event_json JSONB NOT NULL CHECK (jsonb_typeof(signed_event_json) = 'object'),
    command_hash BYTEA NOT NULL CHECK (length(command_hash) = 32),
    resulting_revision BIGINT NOT NULL CHECK (resulting_revision > 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (community_id, owner_pubkey, action_id),
    FOREIGN KEY (community_id, owner_pubkey)
        REFERENCES section_workspaces(community_id, owner_pubkey) ON DELETE CASCADE
);

SELECT attach_community_write_fence('section_workspaces');
SELECT attach_community_write_fence('section_grants');
SELECT attach_community_write_fence('section_key_envelopes');
SELECT attach_community_write_fence('sections');
SELECT attach_community_write_fence('section_assignments');
SELECT attach_community_write_fence('section_actions');
