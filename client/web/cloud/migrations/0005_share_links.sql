-- "Anyone with the link" sharing: a document may have one link token that
-- lets anyone read it without signing in. The owner can copy it again later,
-- so it is stored as is; whoever can read this table can read the documents.
CREATE TABLE share_links (
  document_id TEXT PRIMARY KEY REFERENCES documents(id),
  token TEXT NOT NULL UNIQUE,
  created_by TEXT NOT NULL REFERENCES users(id),
  created_at INTEGER NOT NULL
);
