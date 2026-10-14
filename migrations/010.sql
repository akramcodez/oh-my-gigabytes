BEGIN IMMEDIATE;
-- Only Index::entries() used this index; dropping it speeds every scan insert.
DROP INDEX IF EXISTS entry_size;
PRAGMA user_version=10;
COMMIT;
