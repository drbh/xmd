// The documents API. Every handler resolves the caller's role for the
// document first; authorization never depends on anything the client sends.
import { getServerByName } from "partyserver";
const MAX_TEXT = 1_000_000, MAX_NAME = 200, ROLES = new Set(["editor", "viewer"]);
const ID = /^[A-Za-z0-9_-]{1,64}$/;

export class HttpError extends Error {
  constructor(status, message, extra) { super(message); this.status = status; this.extra = extra; }
}
const json = (body, status = 200, headers = {}) => new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json; charset=utf-8", "cache-control": "no-store", ...headers } });

/** Record the identity and turn any pending invites for its email into ACL rows. */
export async function ensureUser(db, user) {
  const now = Date.now();
  await db.prepare("INSERT INTO users (id, email, name, created_at, last_seen_at) VALUES (?1, ?2, ?3, ?4, ?4) ON CONFLICT(id) DO UPDATE SET email = excluded.email, name = COALESCE(excluded.name, users.name), last_seen_at = excluded.last_seen_at")
    .bind(user.id, user.email, user.name, now).run();
  const pending = await db.prepare("SELECT document_id, role FROM invites WHERE email = ?1").bind(user.email).all();
  const pendingFolders = await db.prepare("SELECT folder_id, role FROM folder_invites WHERE email = ?1").bind(user.email).all();
  if (pending.results.length || pendingFolders.results.length) {
    const statements = pending.results.map(i => db.prepare("INSERT OR REPLACE INTO document_acl (document_id, user_id, role, created_at) VALUES (?1, ?2, ?3, ?4)").bind(i.document_id, user.id, i.role, now));
    statements.push(...pendingFolders.results.map(i => db.prepare("INSERT OR REPLACE INTO folder_acl (folder_id, user_id, role, created_at) VALUES (?1, ?2, ?3, ?4)").bind(i.folder_id, user.id, i.role, now)));
    statements.push(db.prepare("DELETE FROM invites WHERE email = ?1").bind(user.email), db.prepare("DELETE FROM folder_invites WHERE email = ?1").bind(user.email));
    await db.batch(statements);
  }
}

// Roles: the owner may do everything; editors read and write; viewers read.
const best = (...roles) => roles.includes("owner") ? "owner" : roles.includes("editor") ? "editor" : roles.includes("viewer") ? "viewer" : null;
export async function roleOf(db, user, id) {
  const doc = await db.prepare("SELECT id, owner_id, name, text, version, created_at, updated_at, deleted_at, folder_id FROM documents WHERE id = ?1").bind(id).first();
  if (!doc || doc.deleted_at) return { doc: null, role: null };
  if (doc.owner_id === user.id) return { doc, role: "owner" };
  const acl = await db.prepare("SELECT role FROM document_acl WHERE document_id = ?1 AND user_id = ?2").bind(id, user.id).first();
  const viaFolder = doc.folder_id ? await db.prepare("SELECT a.role FROM folder_acl a JOIN folders f ON f.id = a.folder_id WHERE a.folder_id = ?1 AND a.user_id = ?2 AND f.deleted_at IS NULL").bind(doc.folder_id, user.id).first() : null;
  return { doc, role: best(acl?.role, viaFolder?.role) };
}
async function folderRoleOf(db, user, id) {
  const folder = await db.prepare("SELECT id, owner_id, name, created_at, updated_at, deleted_at FROM folders WHERE id = ?1").bind(id).first();
  if (!folder || folder.deleted_at) return { folder: null, role: null };
  if (folder.owner_id === user.id) return { folder, role: "owner" };
  const acl = await db.prepare("SELECT role FROM folder_acl WHERE folder_id = ?1 AND user_id = ?2").bind(id, user.id).first();
  return { folder, role: acl?.role ?? null };
}
const requireRead = ({ doc, role }) => { if (!doc || !role) throw new HttpError(404, "Document not found"); return doc; };
const requireWrite = ({ doc, role }) => { if (!doc || !role) throw new HttpError(404, "Document not found"); if (role === "viewer") throw new HttpError(403, "You can view this document but not edit it"); return doc; };
const requireOwner = ({ doc, role }) => { if (!doc || !role) throw new HttpError(404, "Document not found"); if (role !== "owner") throw new HttpError(403, "Only the owner can do that"); return doc; };
const present = (doc, role) => ({ id: doc.id, name: doc.name, text: doc.text, version: doc.version, updated: doc.updated_at, created: doc.created_at, role, owner: doc.owner_email ?? undefined, folder: doc.folder_id ?? null });

async function body(request) {
  try { return await request.json(); } catch { throw new HttpError(400, "Expected a JSON body"); }
}
function validateDocument(input) {
  if (typeof input.name !== "string" || typeof input.text !== "string") throw new HttpError(400, "name and text are required");
  if (input.name.length > MAX_NAME) throw new HttpError(400, `name is longer than ${MAX_NAME} characters`);
  if (input.text.length > MAX_TEXT) throw new HttpError(413, "Documents are limited to 1 MB");
}

export async function handle(request, env, user) {
  const db = env.DB;
  const url = new URL(request.url);
  const path = url.pathname.replace(/^.*?\/api/, "/api");
  const method = request.method;
  await ensureUser(db, user);

  if (path === "/api/me") return json({ id: user.id, email: user.email, name: user.name });

  if (path === "/api/documents" && method === "GET") {
    const rows = await db.prepare(`
      SELECT d.id, d.owner_id, d.name, d.text, d.version, d.created_at, d.updated_at, d.folder_id, u.email AS owner_email,
             CASE WHEN d.owner_id = ?1 THEN 'owner' WHEN a.role = 'editor' OR fa.role = 'editor' THEN 'editor' ELSE 'viewer' END AS role
      FROM documents d JOIN users u ON u.id = d.owner_id
      LEFT JOIN document_acl a ON a.document_id = d.id AND a.user_id = ?1
      LEFT JOIN folder_acl fa ON fa.folder_id = d.folder_id AND fa.user_id = ?1
      LEFT JOIN folders f ON f.id = d.folder_id AND f.deleted_at IS NULL
      WHERE d.deleted_at IS NULL AND (d.owner_id = ?1 OR a.user_id = ?1 OR (fa.user_id = ?1 AND f.id IS NOT NULL))
      ORDER BY d.updated_at DESC`).bind(user.id).all();
    return json(rows.results.map(r => present(r, r.role)));
  }

  const folderResponse = await folders(db, user, path, method, request);
  if (folderResponse) return folderResponse;

  const m = /^\/api\/documents\/([^/]+)(\/acl)?$/.exec(path);
  if (!m) throw new HttpError(404, "No such endpoint");
  const id = m[1];
  if (!ID.test(id)) throw new HttpError(400, "Invalid document id");
  const access = await roleOf(db, user, id);

  if (!m[2]) {
    if (method === "GET") return json(present(requireRead(access), access.role));
    if (method === "PUT") {
      const input = await body(request);
      const now = Date.now();
      // Only the owner files a document, and only into a folder they own.
      const filing = input.folder !== undefined && (!access.doc || access.role === "owner");
      if (filing && input.folder !== null) {
        if (!ID.test(String(input.folder))) throw new HttpError(400, "Invalid folder id");
        const own = await db.prepare("SELECT id FROM folders WHERE id = ?1 AND owner_id = ?2 AND deleted_at IS NULL").bind(input.folder, user.id).first();
        if (!own) throw new HttpError(404, "Folder not found");
      }
      if (input.text === undefined && access.doc) {
        // Filing only: the text is left alone (a live document's room owns it).
        requireOwner(access);
        if (!filing) throw new HttpError(400, "Nothing to change");
        await db.prepare("UPDATE documents SET folder_id = ?1, updated_at = ?2 WHERE id = ?3").bind(input.folder, now, id).run();
        return json({ id, version: access.doc.version, updated: now, role: "owner", folder: input.folder });
      }
      validateDocument(input);
      if (!access.doc) {
        // First save of a client-created id. The row must not exist at all, or someone else's (deleted) document would be reused.
        const taken = await db.prepare("SELECT owner_id FROM documents WHERE id = ?1").bind(id).first();
        if (taken) throw new HttpError(409, "That document id is already in use");
        await db.prepare("INSERT INTO documents (id, owner_id, name, text, version, created_at, updated_at, folder_id) VALUES (?1, ?2, ?3, ?4, 1, ?5, ?5, ?6)").bind(id, user.id, input.name, input.text, now, filing ? input.folder : null).run();
        return json({ id, version: 1, updated: now, role: "owner", folder: filing ? input.folder : null }, 201);
      }
      const doc = requireWrite(access);
      if (input.version !== undefined && input.version !== doc.version) throw new HttpError(409, "The document changed elsewhere", { current: present(doc, access.role) });
      // The document's room is the single writer: it merges this text into the live state and mirrors it to D1.
      const room = await getServerByName(env.Room, id);
      const response = await room.fetch(new Request(`https://room/${id}`, { method: "PUT", headers: { "content-type": "application/json" }, body: JSON.stringify({ text: input.text, name: input.name }) }));
      if (!response.ok) throw new HttpError(502, "The document could not be updated");
      const saved = await response.json();
      if (filing) await db.prepare("UPDATE documents SET name = ?1, updated_at = ?2, folder_id = ?3 WHERE id = ?4").bind(input.name, now, input.folder, id).run();
      else await db.prepare("UPDATE documents SET name = ?1, updated_at = ?2 WHERE id = ?3").bind(input.name, now, id).run();
      return json({ id, version: saved.version ?? doc.version + 1, updated: now, role: access.role, folder: filing ? input.folder : doc.folder_id ?? null });
    }
    if (method === "DELETE") {
      requireOwner(access);
      await db.prepare("UPDATE documents SET deleted_at = ?1 WHERE id = ?2").bind(Date.now(), id).run();
      return json({ ok: true });
    }
    throw new HttpError(405, "Method not allowed");
  }

  // Sharing: owners manage an ACL of users, plus invites for emails not yet seen.
  return acl(db, user, request, method, { id, ownerId: access.doc?.owner_id, role: access.role, exists: !!(access.doc && access.role),
    aclTable: "document_acl", aclKey: "document_id", inviteTable: "invites", label: "document" });
}

// One ACL implementation for documents and folders.
async function acl(db, user, request, method, target) {
  const { id, ownerId, role, exists, aclTable, aclKey, inviteTable, label } = target;
  if (!exists) throw new HttpError(404, `${label[0].toUpperCase()}${label.slice(1)} not found`);
  if (method === "GET") {
    const owner = await db.prepare("SELECT email, name FROM users WHERE id = ?1").bind(ownerId).first();
    const entries = await db.prepare(`SELECT u.email, u.name, a.role FROM ${aclTable} a JOIN users u ON u.id = a.user_id WHERE a.${aclKey} = ?1 ORDER BY u.email`).bind(id).all();
    const invites = await db.prepare(`SELECT email, role FROM ${inviteTable} WHERE ${aclKey} = ?1 ORDER BY email`).bind(id).all();
    return json({ owner, role, entries: entries.results, invites: invites.results });
  }
  if (role !== "owner") throw new HttpError(403, "Only the owner can do that");
  const input = await body(request);
  const email = String(input.email || "").trim().toLowerCase();
  if (!/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email)) throw new HttpError(400, "A valid email is required");
  if (method === "PUT") {
    if (!ROLES.has(input.role)) throw new HttpError(400, "role must be editor or viewer");
    if (email === user.email) throw new HttpError(400, `You already own this ${label}`);
    const person = await db.prepare("SELECT id FROM users WHERE email = ?1").bind(email).first();
    const now = Date.now();
    if (person) await db.prepare(`INSERT OR REPLACE INTO ${aclTable} (${aclKey}, user_id, role, created_at) VALUES (?1, ?2, ?3, ?4)`).bind(id, person.id, input.role, now).run();
    else await db.prepare(`INSERT OR REPLACE INTO ${inviteTable} (${aclKey}, email, role, invited_by, created_at) VALUES (?1, ?2, ?3, ?4, ?5)`).bind(id, email, input.role, user.id, now).run();
    return json({ email, role: input.role, invited: !person });
  }
  if (method === "DELETE") {
    await db.batch([
      db.prepare(`DELETE FROM ${aclTable} WHERE ${aclKey} = ?1 AND user_id IN (SELECT id FROM users WHERE email = ?2)`).bind(id, email),
      db.prepare(`DELETE FROM ${inviteTable} WHERE ${aclKey} = ?1 AND email = ?2`).bind(id, email),
    ]);
    return json({ ok: true });
  }
  throw new HttpError(405, "Method not allowed");
}

// Folders: owned or shared; only the owner renames, deletes, or shares one.
async function folders(db, user, path, method, request) {
  if (path === "/api/folders" && method === "GET") {
    const rows = await db.prepare(`
      SELECT f.id, f.name, f.updated_at, u.email AS owner_email, CASE WHEN f.owner_id = ?1 THEN 'owner' ELSE a.role END AS role
      FROM folders f JOIN users u ON u.id = f.owner_id LEFT JOIN folder_acl a ON a.folder_id = f.id AND a.user_id = ?1
      WHERE f.deleted_at IS NULL AND (f.owner_id = ?1 OR a.user_id = ?1) ORDER BY f.name`).bind(user.id).all();
    return json(rows.results.map(f => ({ id: f.id, name: f.name, updated: f.updated_at, owner: f.owner_email, role: f.role })));
  }
  const m = /^\/api\/folders\/([^/]+)(\/acl)?$/.exec(path);
  if (!m) return null;
  const id = m[1];
  if (!ID.test(id)) throw new HttpError(400, "Invalid folder id");
  const access = await folderRoleOf(db, user, id);
  if (m[2]) return acl(db, user, request, method, { id, ownerId: access.folder?.owner_id, role: access.role, exists: !!(access.folder && access.role), aclTable: "folder_acl", aclKey: "folder_id", inviteTable: "folder_invites", label: "folder" });
  if (method === "PUT") {
    const input = await body(request);
    const name = String(input.name ?? "").trim().slice(0, MAX_NAME);
    if (!name) throw new HttpError(400, "A folder name is required");
    const now = Date.now();
    if (!access.folder) {
      const taken = await db.prepare("SELECT id FROM folders WHERE id = ?1").bind(id).first();
      if (taken) throw new HttpError(409, "That folder id is already in use");
      await db.prepare("INSERT INTO folders (id, owner_id, name, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?4)").bind(id, user.id, name, now).run();
      return json({ id, name, updated: now, role: "owner", owner: user.email }, 201);
    }
    if (!access.role) throw new HttpError(404, "Folder not found");
    if (access.role !== "owner") throw new HttpError(403, "Only the owner can rename a folder");
    await db.prepare("UPDATE folders SET name = ?1, updated_at = ?2 WHERE id = ?3").bind(name, now, id).run();
    return json({ id, name, updated: now, role: "owner", owner: user.email });
  }
  if (method === "DELETE") {
    if (!access.folder || !access.role) throw new HttpError(404, "Folder not found");
    if (access.role !== "owner") throw new HttpError(403, "Only the owner can delete a folder");
    // Documents stay; they just leave the folder.
    await db.batch([
      db.prepare("UPDATE documents SET folder_id = NULL WHERE folder_id = ?1").bind(id),
      db.prepare("UPDATE folders SET deleted_at = ?1 WHERE id = ?2").bind(Date.now(), id),
    ]);
    return json({ ok: true });
  }
  throw new HttpError(405, "Method not allowed");
}

export { json };
