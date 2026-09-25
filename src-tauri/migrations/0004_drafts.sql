-- Locally auto-saved compose drafts (protection against data loss).
CREATE TABLE drafts (
    id                     INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id             INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    mode                   TEXT    NOT NULL,             -- new|reply|replyAll|forward
    source_message_id      INTEGER REFERENCES messages(id) ON DELETE SET NULL,
    to_addr                TEXT    NOT NULL DEFAULT '',
    cc_addr                TEXT    NOT NULL DEFAULT '',
    bcc_addr               TEXT    NOT NULL DEFAULT '',
    subject                TEXT    NOT NULL DEFAULT '',
    body_html              TEXT    NOT NULL DEFAULT '',
    forward_idx_json       TEXT    NOT NULL DEFAULT '[]',
    attachment_paths_json  TEXT    NOT NULL DEFAULT '[]',
    updated_at             INTEGER NOT NULL
);
