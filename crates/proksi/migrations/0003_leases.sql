-- Leases (e.g. which replica orders a domain's certificate), and the revision of the
-- certificates, so replicas reload them when another one writes a certificate

CREATE TABLE IF NOT EXISTS proksi_locks (
    name TEXT PRIMARY KEY,
    holder TEXT NOT NULL,
    -- Milliseconds since the epoch, by the database's clock
    expires_at BIGINT NOT NULL
);

INSERT INTO proksi_meta (key, value) VALUES ('certificates_revision', 0) ON CONFLICT (key) DO NOTHING;
