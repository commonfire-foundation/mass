//! Versioned, transactional cache upgrades. Read-only opens never migrate.
use anyhow::Result;
use rusqlite::Connection;

pub(super) const VERSION: i64 = 2;
pub(super) fn initialize(db: &Connection, version: i64) -> Result<()> {
    if version == VERSION {
        return Ok(());
    }
    let tx = db.unchecked_transaction()?;
    if version == 0 {
        tx.execute_batch("CREATE TABLE files (key TEXT PRIMARY KEY, source TEXT NOT NULL, path TEXT NOT NULL, fingerprint TEXT NOT NULL, ref TEXT NOT NULL, data TEXT NOT NULL);
            CREATE INDEX files_ref ON files(ref);
            CREATE INDEX files_identity ON files(source,json_extract(data,'$.native_id'),COALESCE(json_extract(data,'$.identity_scope'),''));
            CREATE TABLE state (key TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TRIGGER files_insert AFTER INSERT ON files BEGIN
              INSERT INTO state(key,value) VALUES ('revision','1') ON CONFLICT(key) DO UPDATE SET value=CAST(value AS INTEGER)+1;
            END;
            CREATE TRIGGER files_delete AFTER DELETE ON files BEGIN
              INSERT INTO state(key,value) VALUES ('revision','1') ON CONFLICT(key) DO UPDATE SET value=CAST(value AS INTEGER)+1;
            END;")?;
    } else {
        tx.execute_batch(
            "DROP TRIGGER messages_delete;
            DROP TABLE fts;
            DROP INDEX messages_file;
            ALTER TABLE messages RENAME TO legacy_messages;",
        )?;
    }
    tx.execute_batch("CREATE TABLE duplicates (key TEXT PRIMARY KEY, source TEXT NOT NULL, path TEXT NOT NULL, fingerprint TEXT NOT NULL, canonical TEXT NOT NULL, canonical_fingerprint TEXT NOT NULL);
        CREATE INDEX duplicates_source ON duplicates(source);
        CREATE INDEX duplicates_canonical ON duplicates(canonical);
        CREATE TRIGGER files_delete_duplicates AFTER DELETE ON files BEGIN
            DELETE FROM duplicates WHERE canonical=old.key;
        END;
        ALTER TABLE files ADD COLUMN agent TEXT GENERATED ALWAYS AS (json_extract(data,'$.agent')) VIRTUAL;
        ALTER TABLE files ADD COLUMN workspace TEXT GENERATED ALWAYS AS (COALESCE(json_extract(data,'$.workspace'),'')) VIRTUAL;
        ALTER TABLE files ADD COLUMN activity TEXT GENERATED ALWAYS AS (json_extract(data,'$.activity')) VIRTUAL;
        ALTER TABLE files ADD COLUMN title TEXT GENERATED ALWAYS AS (json_extract(data,'$.title')) VIRTUAL;
        ALTER TABLE files ADD COLUMN cwd TEXT GENERATED ALWAYS AS (json_extract(data,'$.cwd')) VIRTUAL;
        CREATE INDEX files_activity ON files(activity DESC,ref);
        CREATE INDEX files_agent_activity ON files(agent,activity DESC,ref);
        CREATE INDEX files_workspace_activity ON files(workspace,activity DESC,ref);
        CREATE TABLE messages (
            id INTEGER PRIMARY KEY,
            file TEXT NOT NULL,
            ref TEXT NOT NULL,
            native_id TEXT NOT NULL,
            parent TEXT NOT NULL,
            role TEXT NOT NULL,
            text TEXT NOT NULL,
            line INTEGER NOT NULL,
            chunk INTEGER NOT NULL,
            projection INTEGER NOT NULL DEFAULT 0,
            indexed INTEGER NOT NULL DEFAULT 0,
            data TEXT GENERATED ALWAYS AS (json_object(
                'reference',ref,'native_id',native_id,'parent',parent,'role',role,
                'text',text,'line',line,'chunk',chunk,
                'projection',json(CASE WHEN projection THEN 'true' ELSE 'false' END))) VIRTUAL
        );
        CREATE INDEX messages_file ON messages(file);
        CREATE INDEX messages_reference ON messages(file,ref);
        CREATE VIEW search_content AS SELECT m.id,f.title,f.cwd AS project,m.text AS body
            FROM messages m JOIN files f ON f.key=m.file;
        CREATE VIRTUAL TABLE fts USING fts5(title,project,body,content='search_content',content_rowid='id',tokenize='unicode61',prefix='2 3');
        -- Newly parsed chunks are staged until identity/deduplication succeeds.
        -- Only previously indexed chunks may issue an FTS5 deletion command.
        CREATE TRIGGER messages_delete AFTER DELETE ON messages WHEN old.indexed BEGIN
            INSERT INTO fts(fts,rowid,title,project,body)
                SELECT 'delete',old.id,title,cwd,old.text FROM files WHERE key=old.file;
        END;")?;
    if version == 1 {
        tx.execute_batch("INSERT INTO messages(id,file,ref,native_id,parent,role,text,line,chunk,projection,indexed)
            SELECT id,file,ref,json_extract(data,'$.native_id'),json_extract(data,'$.parent'),
                json_extract(data,'$.role'),json_extract(data,'$.text'),json_extract(data,'$.line'),
                json_extract(data,'$.chunk'),COALESCE(json_extract(data,'$.projection'),0),1
            FROM legacy_messages;
            DROP TABLE legacy_messages;
            INSERT INTO fts(fts) VALUES('rebuild');
            INSERT INTO state(key,value) VALUES ('revision','1') ON CONFLICT(key) DO UPDATE SET value=CAST(value AS INTEGER)+1;")?;
    }
    tx.pragma_update(None, "user_version", VERSION)?;
    tx.pragma_update(None, "application_id", 0x4d415353)?;
    tx.commit()?;
    Ok(())
}
