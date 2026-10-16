-- Let's Encrypt account and order data, shared by all replicas

CREATE TABLE IF NOT EXISTS proksi_acme (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
