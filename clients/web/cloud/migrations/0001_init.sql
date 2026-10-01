-- Users are Access identities. Documents belong to one owner; other people
-- reach them through document_acl, or through an invite that becomes an ACL
-- row when that email first signs in.
CREATE TABLE users (
  id TEXT PRIMARY KEY,
  email TEXT NOT NULL UNIQUE,
  name TEXT,
  created_at INTEGER NOT NULL,
  last_seen_at INTEGER NOT NULL
);
CREATE TABLE documents (
  id TEXT PRIMARY KEY,
  owner_id TEXT NOT NULL REFERENCES users(id),
  name TEXT NOT NULL,
  text TEXT NOT NULL,
  version INTEGER NOT NULL DEFAULT 1,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  deleted_at INTEGER
);
CREATE INDEX documents_owner ON documents(owner_id);
CREATE TABLE document_acl (
  document_id TEXT NOT NULL REFERENCES documents(id),
  user_id TEXT NOT NULL REFERENCES users(id),
  role TEXT NOT NULL CHECK (role IN ('editor', 'viewer')),
  created_at INTEGER NOT NULL,
  PRIMARY KEY (document_id, user_id)
);
CREATE INDEX document_acl_user ON document_acl(user_id);
CREATE TABLE invites (
  document_id TEXT NOT NULL REFERENCES documents(id),
  email TEXT NOT NULL,
  role TEXT NOT NULL CHECK (role IN ('editor', 'viewer')),
  invited_by TEXT NOT NULL REFERENCES users(id),
  created_at INTEGER NOT NULL,
  PRIMARY KEY (document_id, email)
);
CREATE INDEX invites_email ON invites(email);
