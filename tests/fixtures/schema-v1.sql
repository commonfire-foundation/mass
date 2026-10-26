-- Frozen schema from the first Rust index. Never use a live cache as a fixture.
CREATE TABLE files (key TEXT PRIMARY KEY, source TEXT NOT NULL, path TEXT NOT NULL, fingerprint TEXT NOT NULL, ref TEXT NOT NULL, data TEXT NOT NULL);
CREATE INDEX files_ref ON files(ref);
CREATE INDEX files_identity ON files(source,json_extract(data,'$.native_id'),COALESCE(json_extract(data,'$.identity_scope'),''));
CREATE TABLE messages (id INTEGER PRIMARY KEY, file TEXT NOT NULL, ref TEXT NOT NULL, data TEXT NOT NULL);
CREATE INDEX messages_file ON messages(file);
CREATE VIRTUAL TABLE fts USING fts5(title,project,body,tokenize='unicode61',prefix='2 3');
CREATE TRIGGER messages_delete AFTER DELETE ON messages BEGIN DELETE FROM fts WHERE rowid=old.id; END;
CREATE TABLE state (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TRIGGER files_insert AFTER INSERT ON files BEGIN
  INSERT INTO state(key,value) VALUES ('revision','1') ON CONFLICT(key) DO UPDATE SET value=CAST(value AS INTEGER)+1;
END;
CREATE TRIGGER files_delete AFTER DELETE ON files BEGIN
  INSERT INTO state(key,value) VALUES ('revision','1') ON CONFLICT(key) DO UPDATE SET value=CAST(value AS INTEGER)+1;
END;
PRAGMA user_version=1;
PRAGMA application_id=1296126803;
