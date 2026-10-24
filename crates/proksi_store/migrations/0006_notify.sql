-- `notify` blocks saved through the admin API (each block's settings as JSON, and the revision
-- it was saved at), and their revision, so replicas reload them when one changes

CREATE TABLE IF NOT EXISTS proksi_notify (
    name TEXT PRIMARY KEY,
    data TEXT NOT NULL,
    version BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

INSERT INTO proksi_meta (key, value) VALUES ('notify_revision', 0) ON CONFLICT (key) DO NOTHING;
