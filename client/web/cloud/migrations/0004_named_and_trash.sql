-- `named` records an explicit rename: until then a document's file name
-- follows its first heading, as a fresh note on disk would be named.
ALTER TABLE documents ADD COLUMN named INTEGER NOT NULL DEFAULT 0;
