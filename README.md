# nuntii

Fast, lightweight desktop mail client for Windows, macOS and Linux. Built with Tauri 2, React and Rust, and free of license fees for commercial use.

**Status:** MVP round 1 – Synology MailPlus / generic IMAP and Gmail (OAuth2). Microsoft 365 (Graph) and Exchange EWS come later.

## Features

- Multiple accounts at once (IMAP/SMTP with password, Gmail via OAuth2)
- Folder tree, virtualized message list, conversation view (threading via `References`/`In-Reply-To`)
- Read, reply, reply all, forward, attachments (receiving and sending), signatures per account
- Local full-text search (tantivy), e.g. `from:anna invoice`, `subject:offer`
- Offline reading: the last 30 days in the inbox and Sent are preloaded; everything else is cached once opened
- Outbox: mail is queued first and retried automatically if sending fails
- Keyboard shortcuts: `j`/`k` next/previous, `r` reply, `a` reply all, `f` forward, `c` new, `s` flag, `u` read/unread, `Del` delete, `/` search

## Security

- Passwords, refresh tokens and the Google client secret are stored **only** in the OS keychain (Windows Credential Manager, macOS Keychain, Linux Secret Service), never in the database or in files.
- Encrypted connections only: implicit TLS or mandatory STARTTLS, with no plaintext fallback. Certificates are validated against the OS trust store.
- HTML mail is sanitized with `ammonia`: no scripts, event handlers, forms or iframes. Remote images, CSS `url()` loads and tracking pixels are blocked until the user allows them, either for one mail or permanently per sender. Rendering happens in an iframe without `allow-scripts` and with its own CSP.

## Development

Requirements: Node.js 20+, Rust (stable), and the [Tauri prerequisites](https://tauri.app/start/prerequisites/) for your OS.

```sh
npm install
npm run tauri dev        # start the app in development mode
npm test                 # Rust tests (MIME, sanitizer, threading, search, store)
npm run build            # type-check + frontend build
npm run tauri build      # installer/bundle
npm run check:licenses   # npm dependencies: permissive licenses only
cargo deny --manifest-path src-tauri/Cargo.toml check licenses   # same for Rust
```

App data (SQLite database `nuntii.db`, search index `index/`) is stored in the OS app data directory under `app.nuntii.desktop`.

## Setting up accounts

### Synology MailPlus / IMAP

Enter the NAS host name, e.g. `nas.example.com`. The defaults are IMAP 993 (SSL/TLS) and SMTP 465 (SSL/TLS); STARTTLS works on 143/587. If the NAS uses a self-signed certificate, install its certificate or CA in the OS as trusted. nuntii does not skip certificate validation.

### Gmail

Gmail works only via OAuth2. It needs your own OAuth client:

1. In the [Google Cloud Console](https://console.cloud.google.com/), create a project and enable the **Gmail API**.
2. Set up the **OAuth consent screen** (type "External"). While it is in testing mode, add your own Google accounts as **test users**.
3. Under **Credentials**, create an **OAuth client ID** of type **Desktop app**.
4. In nuntii, go to *Settings → Gmail* and enter the client ID and client secret.
5. *Add account → Gmail → Sign in with Google*: sign-in happens in the browser and the redirect goes to a local loopback port.

For use beyond your own test users, Google requires OAuth app verification, because the `https://mail.google.com/` scope is a restricted scope.

## Architecture

```
src/                     React UI (shadcn/ui, Tailwind, TanStack Query/Virtual)
  lib/api.ts             typed wrappers around the Tauri commands
  features/…             sidebar, message list, reading pane, compose, settings
src-tauri/src/
  provider/              MailProvider trait + IMAP/SMTP implementation (async-imap, lettre)
  auth/                  OS keychain, Google OAuth2 (PKCE + loopback)
  store/                 SQLite (sqlx) – accounts, folders, messages, bodies, outbox
  sync/                  poll-based sync per account (UIDVALIDITY/UIDNEXT, CONDSTORE)
  search/                tantivy index
  mime/                  parsing (mail-parser), building (lettre)
  html/                  sanitizing (ammonia)
  commands/              Tauri commands (frontend ↔ backend)
```

The UI works only against the `MailProvider` trait and the local cache, so it does not depend on any protocol. Microsoft Graph will become a second implementation of the trait.

Changes reach the frontend as events: `mail://changed`, `account://status` and `sync://progress`.

## Known limitations (round 1)

- Polling every 2 minutes; IMAP IDLE / push is planned for phase 2
- The first sync is limited to a time window (default 90 days, configurable); older mail is not synced
- Compose supports plain text only, with no HTML editor yet
- Moving mail between accounts is not supported
