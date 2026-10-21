-- Portable SQL: used by both sqlite and postgres

CREATE TABLE IF NOT EXISTS proksi_meta (
    key TEXT PRIMARY KEY,
    value BIGINT NOT NULL
);

INSERT INTO proksi_meta (key, value) VALUES ('revision', 0) ON CONFLICT (key) DO NOTHING;

CREATE TABLE IF NOT EXISTS proksi_routes (
    host TEXT PRIMARY KEY,
    schema_version BIGINT NOT NULL,
    data TEXT NOT NULL,
    updated_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS proksi_replicas (
    id TEXT PRIMARY KEY,
    data TEXT NOT NULL,
    expires_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS proksi_certificates (
    domain TEXT PRIMARY KEY,
    data TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS proksi_challenges (
    domain TEXT PRIMARY KEY,
    token TEXT NOT NULL,
    proof TEXT NOT NULL,
    expires_at BIGINT NOT NULL
);
