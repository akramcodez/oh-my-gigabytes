BEGIN IMMEDIATE;
CREATE TABLE IF NOT EXISTS volume_samples(id INTEGER PRIMARY KEY,at INTEGER NOT NULL,root BLOB NOT NULL,capacity INTEGER NOT NULL,available INTEGER NOT NULL,indexed_allocated INTEGER,scan_state TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS sample_at ON volume_samples(at);
CREATE TABLE IF NOT EXISTS growth_snapshots(sample_id INTEGER NOT NULL REFERENCES volume_samples(id) ON DELETE CASCADE,path BLOB NOT NULL,allocated INTEGER NOT NULL,PRIMARY KEY(sample_id,path));
CREATE TABLE IF NOT EXISTS service_state(singleton INTEGER PRIMARY KEY CHECK(singleton=1),nonce TEXT NOT NULL,pid INTEGER NOT NULL,started INTEGER NOT NULL,heartbeat INTEGER NOT NULL,phase TEXT NOT NULL,failures INTEGER NOT NULL,next_run INTEGER,last_scan INTEGER,last_error TEXT);
CREATE TABLE IF NOT EXISTS notification_state(volume TEXT PRIMARY KEY,phase TEXT NOT NULL,since INTEGER NOT NULL,last_notified INTEGER);
PRAGMA user_version=4;
COMMIT;
