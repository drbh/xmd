// One Durable Object per document: the authoritative live text, relayed to
// every connected editor over WebSockets (Yjs sync + awareness via
// y-partyserver) and written back to D1 so the rest of the API, sharing, and
// export keep reading the `documents` table. While a room exists it is the
// only writer for its document.
import { YServer } from "y-partyserver";
import { encodeStateAsUpdate, applyUpdate } from "yjs";
import { roleOf } from "./api.js";

const TEXT = "text"; // the Y.Text holding the note
const STATE = "yjs-state"; // storage key prefix for the encoded document, chunked under the 2 MB value limit
const CHUNK = 1_000_000;

export class Room extends YServer {
  static options = { hibernate: true };
  static callbackOptions = { debounceWait: 1500, debounceMaxWait: 8000 };

  get text() { return this.document.getText(TEXT); }

  // Load the CRDT state if we have one; otherwise seed it from the D1 row.
  async onLoad() {
    const stored = await this.readState();
    if (stored) { applyUpdate(this.document, stored, "storage"); return; }
    const row = await this.env.DB.prepare("SELECT text FROM documents WHERE id = ?1 AND deleted_at IS NULL").bind(this.name).first();
    if (row?.text) this.document.transact(() => this.text.insert(0, row.text), "storage");
  }

  // Persist the CRDT and mirror the plain text into D1 with a new version.
  async onSave() {
    await this.writeState(encodeStateAsUpdate(this.document));
    const text = this.text.toString();
    await this.env.DB.prepare("UPDATE documents SET text = ?1, name = COALESCE(?2, name), version = version + 1, updated_at = ?3 WHERE id = ?4 AND text != ?1")
      .bind(text, titleOf(text), Date.now(), this.name).run();
  }
  async readState() {
    const meta = await this.ctx.storage.get(`${STATE}:count`);
    if (!meta) return null;
    const parts = await this.ctx.storage.get(Array.from({ length: meta }, (_, i) => `${STATE}:${i}`));
    const out = new Uint8Array([...parts.values()].reduce((n, p) => n + p.byteLength, 0));
    let at = 0;
    for (let i = 0; i < meta; i++) { const p = new Uint8Array(parts.get(`${STATE}:${i}`)); out.set(p, at); at += p.byteLength; }
    return out;
  }
  async writeState(bytes) {
    const entries = {};
    const count = Math.max(1, Math.ceil(bytes.byteLength / CHUNK));
    for (let i = 0; i < count; i++) entries[`${STATE}:${i}`] = bytes.slice(i * CHUNK, (i + 1) * CHUNK).buffer;
    entries[`${STATE}:count`] = count;
    await this.ctx.storage.put(entries);
  }

  // Check again inside the room: a permission change can race the upgrade.
  async onConnect(connection, context) {
    await this.ctx.blockConcurrencyWhile(async () => {
      const userId = context.request.headers.get("x-xmd-user-id");
      const { role } = userId ? await roleOf(this.env.DB, { id: userId }, this.name) : { role: null };
      connection.setState({ userId, role, email: context.request.headers.get("x-xmd-email") || "" });
      if (!role) { this.revoke(connection); return; }
      return super.onConnect(connection, context);
    });
  }
  isReadOnly(connection) { return connection.state?.role !== "owner" && connection.state?.role !== "editor"; }
  // A closing socket can still have queued frames. Exclude it from both
  // incoming sync messages and the base class's outgoing broadcasts.
  onMessage(connection, message) {
    if (connection.state?.role) return super.onMessage(connection, message);
  }
  *getConnections(tag) {
    for (const connection of super.getConnections(tag)) if (connection.state?.role) yield connection;
  }
  revoke(connection) {
    connection.setState({ ...connection.state, role: null });
    connection.close(4003, "Document access changed");
  }
  async refreshAccess() {
    await this.ctx.blockConcurrencyWhile(async () => {
      for (const connection of super.getConnections()) {
        const { userId, role: previous } = connection.state || {};
        const { role } = userId ? await roleOf(this.env.DB, { id: userId }, this.name) : { role: null };
        if (!role || role !== previous) this.revoke(connection);
      }
    });
  }

  // Non-live writes (a REST PUT while a room exists) come through here so
  // there is one writer. The edit is applied as a replacement of the text.
  async onRequest(request) {
    if (request.method === "POST" && new URL(request.url).pathname.endsWith("/access")) {
      await this.refreshAccess();
      return Response.json({ ok: true });
    }
    if (request.method === "PUT") {
      const { text, name } = await request.json();
      this.document.transact(() => { const current = this.text.toString(); if (current !== text) { this.text.delete(0, current.length); this.text.insert(0, text); } }, "api");
      await this.onSave();
      const row = await this.env.DB.prepare("SELECT version, updated_at FROM documents WHERE id = ?1").bind(this.name).first();
      return Response.json({ id: this.name, version: row?.version, updated: row?.updated_at, name });
    }
    if (request.method === "GET") return Response.json({ id: this.name, text: this.text.toString(), connections: [...this.getConnections()].length });
    return new Response("Method not allowed", { status: 405 });
  }
}

// The name follows the first heading, as in the app; without one it is kept.
function titleOf(text) {
  const heading = text.split("\n").find(l => /^#+\s+\S/.test(l));
  return heading ? heading.replace(/^#+\s+/, "").replace(/\s+:\w+$/, "").trim().slice(0, 200) : null;
}
