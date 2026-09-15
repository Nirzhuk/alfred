CREATE TABLE IF NOT EXISTS shares (
  id TEXT PRIMARY KEY NOT NULL,
  request_id TEXT NOT NULL,
  owner_hash TEXT NOT NULL,
  digest TEXT NOT NULL,
  payload TEXT,
  revoked INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  UNIQUE(owner_hash, request_id)
);
CREATE INDEX IF NOT EXISTS shares_digest ON shares(digest);
CREATE TRIGGER IF NOT EXISTS shares_storage_cap
BEFORE INSERT ON shares
WHEN (SELECT COUNT(*) FROM shares) >= 100000
  OR (SELECT COUNT(*) FROM shares WHERE revoked = 0) >= 10000
  OR (SELECT COALESCE(SUM(length(CAST(payload AS BLOB))), 0) FROM shares) + length(CAST(COALESCE(NEW.payload, '') AS BLOB)) > 2621440000
BEGIN
  SELECT RAISE(ABORT, 'publication storage cap reached');
END;
