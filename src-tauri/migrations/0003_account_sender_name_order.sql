-- display_name is now the account name shown in the app; sender_name goes into the From header.
ALTER TABLE accounts ADD COLUMN sender_name TEXT NOT NULL DEFAULT '';
ALTER TABLE accounts ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0;
UPDATE accounts SET
    sender_name = CASE WHEN lower(display_name) = lower(email) THEN '' ELSE display_name END,
    sort_order = id;
