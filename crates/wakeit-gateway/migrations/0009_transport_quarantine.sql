ALTER TABLE outbox ADD COLUMN quarantined_at timestamptz;
ALTER TABLE outbox ADD COLUMN quarantine_reason text;
CREATE INDEX outbox_pending_unquarantined ON outbox(id)
 WHERE published_at IS NULL AND quarantined_at IS NULL;
