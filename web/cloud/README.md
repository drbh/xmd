# Hosted docs app on Cloudflare

Everything Cloudflare-specific lives in this folder. The app in `apps/docs`
does not depend on it: delete this folder and the static site still works,
saving documents in the browser.

```text
worker/       Worker: serves web/dist and the /api accounts API (D1 + Access)
client/       backend.js, the browser module implementing the app's DocumentBackend
migrations/   D1 schema
test/         API tests run through `wrangler dev` with development identities
build.mjs     builds the site and copies client/backend.js to dist/docs/backend.js
```

## How the app finds the backend

At startup the app fetches `./backend.js` next to itself. The static build has
no such file, so it stays local. `build.mjs` adds it for this deployment; it
implements `list`, `save`, `delete`, `subscribe`, `acl`, `signIn`, and `signOut`
against `/api`. If `/api/me` says sign-in is not configured or the visitor is
signed out, the app keeps working locally and offers **Sign in** on the home
screen. After signing in, documents saved in the browser can be moved to the
account from the home screen.

## Identity: Cloudflare Access

The Worker does not implement OAuth. A Cloudflare Access application protects
`/api/*` and forwards a signed JWT, which `worker/auth.js` verifies against the
team's public keys (audience, issuer, expiry). One-time setup:

1. Zero Trust → Access → Applications → Add → Self-hosted.
2. Domain: the Worker's hostname; path `api`. Identity provider: Google (or any).
3. Policy: allow the emails or domain you want. Copy the application's
   **Audience (AUD) tag** and the team domain (`<team>.cloudflareaccess.com`).
4. Put them in `wrangler.toml` `[vars]` and deploy again.

Until that is done, the API answers 503 and the deployed app runs locally.

## Data model and authorization

`users` (Access identities), `documents` (owner, text, `version`), `document_acl`
(`editor` | `viewer`), and `invites` (email + role, converted to ACL rows the
first time that email signs in). The owner may read, edit, delete, and share;
editors read and edit; viewers read. Every handler resolves the caller's role
in D1 first; the client never decides permissions. Saves carry the document's
`version` and get **409** when it changed elsewhere, which the app reports and
stops saving that document until reloaded.

## Live editing

`worker/room.js` is a Durable Object per document (`Room`, SQLite-backed,
WebSocket hibernation) built on `y-partyserver`: the note is a Yjs text,
edits are relayed to every connection, presence carries names and carets,
and the CRDT state is kept in the object's storage. `onSave` mirrors the text
into the `documents` row (new `version`), so listing, sharing, export, and the
query console keep reading D1. While a room exists it is the only writer: the
REST `PUT` is forwarded to it and merged into the live text.

The Worker resolves the caller's role before the WebSocket upgrade
(`onBeforeConnect`) and the room drops document updates from viewers. The
browser side is `client/live.js`, loaded on demand by `backend.js` when a
cloud document opens (Yjs is a separate chunk); it binds the app's editor
delta API to the shared text and a per-user Yjs undo history.

`npm test` covers the API through `wrangler dev` and drives two editors plus a
viewer in real browsers (`test/live.spec.mjs`).

## Offline

`build.mjs` adds the backend module and its Yjs chunk to the service worker's
precache, so the hosted app also opens offline. `client/backend.js` remembers
the last library and account; without a network it serves that, queues saves
in an outbox that is retried when the network returns, and reports
`offline`. Live documents keep their Yjs state in IndexedDB (`y-indexeddb`):
a document opened online before can be edited offline and the room merges
the edits on reconnect through the sync handshake; one never opened here is
read-only until it has been, since seeding it locally would duplicate the
room's text.

## Commands

```sh
npm --prefix web/cloud run dev       # local Worker + local D1, DEV_AUTH identities (X-Dev-User header)
npm --prefix web/cloud test          # API tests and live-editing browser tests through wrangler dev
npm --prefix web/cloud run deploy    # build, apply remote migrations, deploy
```

`wrangler dev --var DEV_AUTH:1` replaces Access with a fixed identity; it is
never set in `wrangler.toml`, so production always requires Access.
