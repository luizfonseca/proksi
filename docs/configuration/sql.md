---
description: Use SQLite or Postgres to store certificates and routes.
---

# SQLite / Postgres

Like [Redis](redis.md), SQLite and Postgres can store certificates, ACME challenges and routes, shared by every Proksi instance using the same database.

These stores are optional and need Proksi to be built with the `store-sqlite` and/or `store-postgres` cargo features:

```bash
cargo install proksi --features store-sqlite,store-postgres
```

## Configuration

{% code title="proksi.hcl" lineNumbers="true" %}
```hcl
store {
  store_type = "postgres" # or "sqlite"
  database_url = "postgres://user:password@localhost:5432/proksi"
  # database_url = "sqlite:///var/lib/proksi/proksi.db?mode=rwc"

  # How often (in seconds) routes are synced from the store (default: 10)
  sync_interval_secs = 10
}
```
{% endcode %}

Proksi creates (and updates) its tables on startup. For SQLite, `?mode=rwc` creates the database file if it doesn't exist.

## Routes

Routes are stored in the `proksi_routes` table, one row per host (lowercase), with the route in the `data` column as JSON:

```json
{ "schema_version": 1, "route": { "host": "example.com", "upstreams": [{ "ip": "10.0.0.1", "port": 3000 }] } }
```

The `route` has the same format as a route in the configuration file. Every change must increment the revision in the same transaction, otherwise instances won't see it:

```sql
BEGIN;
INSERT INTO proksi_routes (host, schema_version, data, updated_at)
VALUES ('example.com', 1, '{"schema_version":1,"route":{...}}', 0)
ON CONFLICT (host) DO UPDATE SET data = excluded.data, updated_at = excluded.updated_at;
UPDATE proksi_meta SET value = value + 1 WHERE key = 'revision';
COMMIT;
```

Each instance reports the revision it applied in `proksi_replicas`. As with Redis, routes from the store win over docker labels and the configuration file, and the routes from the last sync are kept if the database can't be reached.

ACME account data is kept on disk (`paths.lets_encrypt`); certificates themselves are stored in the database.
