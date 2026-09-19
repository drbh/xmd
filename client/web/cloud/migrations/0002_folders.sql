-- Folders group documents. Sharing a folder shares every document in it; a
-- document's effective role is the best of its own ACL and its folder's.
CREATE TABLE folders (
  id TEXT PRIMARY KEY,
  owner_id TEXT NOT NULL REFERENCES users(id),
  name TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  deleted_at INTEGER
);
CREATE INDEX folders_owner ON folders(owner_id);
ALTER TABLE documents ADD COLUMN folder_id TEXT REFERENCES folders(id);
CREATE INDEX documents_folder ON documents(folder_id);
CREATE TABLE folder_acl (
  folder_id TEXT NOT NULL REFERENCES folders(id),
  user_id TEXT NOT NULL REFERENCES users(id),
  role TEXT NOT NULL CHECK (role IN ('editor', 'viewer')),
  created_at INTEGER NOT NULL,
  PRIMARY KEY (folder_id, user_id)
);
CREATE INDEX folder_acl_user ON folder_acl(user_id);
CREATE TABLE folder_invites (
  folder_id TEXT NOT NULL REFERENCES folders(id),
  email TEXT NOT NULL,
  role TEXT NOT NULL CHECK (role IN ('editor', 'viewer')),
  invited_by TEXT NOT NULL REFERENCES users(id),
  created_at INTEGER NOT NULL,
  PRIMARY KEY (folder_id, email)
);
CREATE INDEX folder_invites_email ON folder_invites(email);
