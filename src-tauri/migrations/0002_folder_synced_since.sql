-- Oldest date (unix) up to which a folder has been synced; lets a larger sync window backfill.
ALTER TABLE folders ADD COLUMN synced_since INTEGER;
