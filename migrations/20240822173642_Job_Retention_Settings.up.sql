ALTER TABLE job ADD COLUMN settings BLOB NOT NULL DEFAULT X'0C'; -- can't use `(jsonb('{}'))` and `quote(jsonb('{}'))` == X'0C';
