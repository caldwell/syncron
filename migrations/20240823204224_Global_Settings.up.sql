CREATE TABLE settings (
       key TEXT PRIMARY KEY NOT NULL,
       value BLOB NOT NULL,

       UNIQUE(key)
) STRICT;
CREATE INDEX by_key on settings ( key );
