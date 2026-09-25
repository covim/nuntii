CREATE TABLE accounts (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    kind          TEXT    NOT NULL,              -- 'imap' | 'gmail'
    display_name  TEXT    NOT NULL,
    email         TEXT    NOT NULL,
    imap_host     TEXT    NOT NULL,
    imap_port     INTEGER NOT NULL,
    imap_security TEXT    NOT NULL,              -- 'tls' | 'starttls'
    smtp_host     TEXT    NOT NULL,
    smtp_port     INTEGER NOT NULL,
    smtp_security TEXT    NOT NULL,
    username      TEXT    NOT NULL,
    signature_id  INTEGER REFERENCES signatures(id) ON DELETE SET NULL,
    created_at    INTEGER NOT NULL
);

CREATE TABLE folders (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id    INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    remote_name   TEXT    NOT NULL,
    display_name  TEXT    NOT NULL,
    delimiter     TEXT,
    role          TEXT,                          -- inbox|sent|drafts|trash|archive|junk
    selectable    INTEGER NOT NULL DEFAULT 1,
    uidvalidity   INTEGER,
    uidnext       INTEGER,
    highestmodseq INTEGER,
    last_exists   INTEGER,
    last_sync     INTEGER,
    UNIQUE (account_id, remote_name)
);

CREATE TABLE threads (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id   INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    subject_norm TEXT
);

CREATE TABLE thread_refs (
    account_id     INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    message_id_hdr TEXT    NOT NULL,
    thread_id      INTEGER NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
    PRIMARY KEY (account_id, message_id_hdr)
);

CREATE TABLE messages (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id      INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    folder_id       INTEGER NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
    uid             INTEGER NOT NULL,
    message_id_hdr  TEXT,
    in_reply_to     TEXT,
    references_hdr  TEXT,
    thread_id       INTEGER REFERENCES threads(id) ON DELETE SET NULL,
    subject         TEXT    NOT NULL DEFAULT '',
    from_json       TEXT    NOT NULL DEFAULT '[]',
    to_json         TEXT    NOT NULL DEFAULT '[]',
    cc_json         TEXT    NOT NULL DEFAULT '[]',
    date            INTEGER NOT NULL,
    snippet         TEXT    NOT NULL DEFAULT '',
    seen            INTEGER NOT NULL DEFAULT 0,
    flagged         INTEGER NOT NULL DEFAULT 0,
    answered        INTEGER NOT NULL DEFAULT 0,
    draft           INTEGER NOT NULL DEFAULT 0,
    has_attachments INTEGER NOT NULL DEFAULT 0,
    size            INTEGER NOT NULL DEFAULT 0,
    body_state      TEXT    NOT NULL DEFAULT 'none', -- none|fetched
    UNIQUE (folder_id, uid)
);
CREATE INDEX idx_messages_folder_date ON messages(folder_id, date DESC);
CREATE INDEX idx_messages_thread ON messages(thread_id);
CREATE INDEX idx_messages_msgid ON messages(account_id, message_id_hdr);

CREATE TABLE message_bodies (
    message_id INTEGER PRIMARY KEY REFERENCES messages(id) ON DELETE CASCADE,
    raw        BLOB    NOT NULL,
    text_plain TEXT
);

CREATE TABLE attachments (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    message_id INTEGER NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    idx        INTEGER NOT NULL,                 -- position in mail-parser's attachment list
    filename   TEXT    NOT NULL,
    mime       TEXT    NOT NULL,
    size       INTEGER NOT NULL,
    content_id TEXT,
    inline     INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_attachments_message ON attachments(message_id);

CREATE TABLE signatures (
    id        INTEGER PRIMARY KEY AUTOINCREMENT,
    name      TEXT NOT NULL,
    body_text TEXT NOT NULL
);

CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE outbox (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id  INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    raw         BLOB    NOT NULL,
    env_from    TEXT    NOT NULL,
    env_to_json TEXT    NOT NULL,
    reply_to_message_id INTEGER,
    status      TEXT    NOT NULL DEFAULT 'pending', -- pending|failed
    error       TEXT,
    created_at  INTEGER NOT NULL
);

CREATE TABLE trusted_senders (
    address TEXT PRIMARY KEY                      -- remote images allowed for this sender
);
