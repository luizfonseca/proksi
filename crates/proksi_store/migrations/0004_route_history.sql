-- Versions and history of hosts. Portable SQL: used by both sqlite and postgres.
-- Hosts already in the store start without history: their first change records it.

CREATE TABLE IF NOT EXISTS proksi_route_versions (
    host TEXT NOT NULL,
    revision BIGINT NOT NULL,
    schema_version BIGINT NOT NULL,
    data TEXT NOT NULL,
    created_at BIGINT NOT NULL,
    author TEXT,
    PRIMARY KEY (host, revision)
);

CREATE TABLE IF NOT EXISTS proksi_route_changes (
    host TEXT NOT NULL,
    revision BIGINT NOT NULL,
    created_at BIGINT NOT NULL,
    author TEXT,
    kind TEXT NOT NULL,
    target BIGINT,
    PRIMARY KEY (host, revision)
);

-- The active version of each host (0: none, or saved before versions were kept) and the
-- revision of its last change
CREATE TABLE IF NOT EXISTS proksi_route_state (
    host TEXT PRIMARY KEY,
    version BIGINT NOT NULL,
    changed BIGINT NOT NULL
);
