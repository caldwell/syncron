CREATE TABLE user (
       user_id INTEGER PRIMARY KEY ASC NOT NULL,
       name TEXT NOT NULL,

       UNIQUE(name)
) STRICT;
CREATE INDEX by_name on user ( name );

CREATE TABLE job (
       job_id INTEGER PRIMARY KEY ASC NOT NULL,
       user_id INTEGER NOT NULL,
       id TEXT NOT NULL,
       name TEXT NOT NULL,

       UNIQUE(user_id, id),
       FOREIGN KEY (user_id) REFERENCES user (user_id)
) STRICT;

CREATE TABLE run (
       run_id INTEGER PRIMARY KEY ASC NOT NULL,
       job_id INTEGER NOT NULL,
       client_id TEXT,
       cmd TEXT NOT NULL,
       env TEXT NOT NULL,
       log TEXT NOT NULL,
       start INTEGER NOT NULL,
       end INTEGER,
       heartbeat INTEGER,
       status TEXT,

       UNIQUE(job_id, start),
       UNIQUE(client_id),
       FOREIGN KEY (job_id) REFERENCES job (job_id)
) STRICT;
CREATE INDEX by_job_id_start on run ( job_id, start );
CREATE INDEX by_client_id on run ( client_id );
