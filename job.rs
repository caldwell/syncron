// Copyright © 2022 David Caldwell <david@porkrind.org>

use std::error::Error;
use std::ffi::OsString;
use std::fs::File;
use std::io::Write;
use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;

use reqwest::{header,Url};

use crate::serve;
use crate::db::Db;
use crate::maybe_utf8::MaybeUTF8;
use crate::wrap;

#[derive(Debug)]
pub struct ClientJob {
    id:       String,
    timeout:  Option<std::time::Duration>,
    cmd:      String,
    api:      Api,
}

impl ClientJob {
    pub async fn new(server_url: Url, user: &str, name: &str, id: Option<&str>, timeout: Option<std::time::Duration>, cmd: &str) -> Result<ClientJob, Box<dyn Error>> {
        let mut env=vec![];
        for (k, v) in std::env::vars_os() {
            env.push((MaybeUTF8::new(k),MaybeUTF8::new(v)));
        }
        let api = Api::new(server_url)?;
        let resp: serve::CreateRunResp = serde_json::from_str(&api.post("/run/create", &serde_json::to_string(&serve::CreateRunReq{ user:user.to_string(), name:name.to_string(), id:id.map(|i|i.to_string()), cmd:cmd.to_string(), env:env })?.as_bytes()).await?)?;
        Ok(ClientJob { id:resp.id, timeout:timeout, cmd:cmd.to_string(), api: api })
    }

    pub async fn run(&self) -> Result<(), Box<dyn Error>> {
        use tokio::process::Command;

        let shell = match (std::env::var("SYNCRON_SHELL"), std::env::var("SHELL"), std::env::args().nth(0)) {
            (Ok(sh), _,      _)                    => sh,
            (_,      Ok(sh), Some(me)) if sh != me => sh, // Prevent recursion
            (_,      Ok(sh), None)                 => sh, // Probably will never happen
            (_,      _,      _)                    => "/bin/sh".to_string(),
        };
        let mut child = Command::new(shell).args([OsString::from("-c"), OsString::from(&self.cmd)])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;

        trace!("Spawned child {:?}", child);

        let outpiper = ClientJob::copy_output(self.api.clone(), child.stdout.take().unwrap(), self.id.clone(), serve::OutKind::Stdout);
        let errpiper = ClientJob::copy_output(self.api.clone(), child.stderr.take().unwrap(), self.id.clone(), serve::OutKind::Stderr);
        let pipers = async { tokio::join!(outpiper, errpiper) };
        let heartbeat = async {//|| -> Result<(), ()> {
            let now = std::time::Instant::now();
            loop {
                trace!("Waiting 1 second");
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                if self.timeout.is_some() && now.elapsed() > self.timeout.unwrap() {
                    trace!("Timed out");
                    return Err(now.elapsed());
                }
                trace!("Sending Hearbeat");
                let _resp = self.api.post(&format!("/run/{}/heartbeat", self.id), &[]).await;
            }
            #[allow(unreachable_code)] Ok(()) // if I remove this, it errs.
        };
        let mut timeout = false;
        loop {
            tokio::select! {
                _            = pipers    => { break; },
                Err(elapsed) = heartbeat => {
                    trace!("Timeout reached after {:?}! Killing child {:?}", elapsed, child);
                    timeout = true;
                    child.kill().await?;
                    break;
                },
            }
        }
        let exitcode = child.wait().await?;
        let status = match (exitcode.code(), exitcode.signal(), exitcode.core_dumped(), timeout) {
            (_,          _,         _,     true)  => serve::ExitStatus::ClientTimeout,
            (Some(code), _,         _,     _)     => serve::ExitStatus::Exited(code),
            (_,          Some(sig), false, _)     => serve::ExitStatus::Signal(sig),
            (_,          Some(sig), true,  _)     => serve::ExitStatus::CoreDump(sig),
            (None,       None,      _,     _)     => panic!("Can't happen"),
        };
        self.api.post(&format!("/run/{}/complete", self.id), &serde_json::to_string(&status)?.as_bytes()).await?;
        Ok(())
    }

    async fn copy_output<T: tokio::io::AsyncRead+Unpin >(api: Api, mut from: T, id: String, kind: serve::OutKind)/* -> Result<(),()> */{
        use tokio::io::AsyncReadExt;
        let mut buffer = [0; 4096];
        loop {
            if let Ok(read) = from.read(&mut buffer).await {
                if read == 0 { break }
                let _resp = api.post(&format!("/run/{}/{}", id, kind), &buffer[0..read]).await;
            } else { break }
        }
    }

}

#[derive(Debug,Clone)]
pub struct Api {
    server: Url,
    ua:     reqwest::Client,
}

impl Api {
    fn new(server_url: Url) -> Result<Api, Box<dyn Error>> {
        let mut fake_browser_headers = header::HeaderMap::new();
        fake_browser_headers.insert("accept",           header::HeaderValue::from_static("application/json"));
        let client = reqwest::Client::builder()
            .connection_verbose(true)
            .default_headers(fake_browser_headers)
            .build()?;

        Ok(Api {
            server: server_url,
            ua: client,
        })
    }

    pub async fn post(&self, path: &str, body: &[u8]) -> Result<String, Box<dyn Error>> {
        let bod: Vec<u8> = body.into();
        let resp = self.ua.post(self.server.join(path)?)
            .body(bod)
            .send()
            .await?;

        use std::os::unix::ffi::OsStringExt;
        trace!("API: {} <- {:?}", self.server.join(path)?, OsString::from_vec(body.to_vec()).to_string_lossy());
        Ok(resp.text().await?)
    }

    pub async fn get(&self, path: &str) -> Result<String, Box<dyn Error>> {
        let resp = self.ua.get(self.server.join(path)?)
            .send()
            .await?;

        let resp_str = resp.text().await?;
        trace!("API: {} -> {}", self.server.join(path)?, resp_str);
        Ok(resp_str)
    }
}

#[derive(Debug, Clone)]
pub struct ServerJob {
    pub user: String,
    pub id: String,
    pub name: String,
    pub job_id: i64,
    pub db: Db,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct JobInfo {
    pub name: String,
}

#[derive(Debug)]
pub struct ServerRun {
    pub job: ServerJob,
    pub date: chrono::DateTime<chrono::Local>,
    pub run_id: String,
    pub run_db_id: i64,
    pub client_id: Option<u128>,
    pub log_path: PathBuf,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct ServerRunInfo {
    pub cmd:    String,
    pub env:    Vec<(MaybeUTF8,MaybeUTF8)>,
    pub end:    Option<chrono::DateTime<chrono::Local>>,
    pub status: Option<serve::ExitStatus>,
}

pub fn slug(st: &str) -> String {
    let mut slug = st.replace(|ch: char| !ch.is_ascii_alphanumeric(), "-");
    slug.make_ascii_lowercase();
    slug.split('-').filter(|s| !s.is_empty()).intersperse("-").collect::<String>()
}

async fn user_id(db: &Db, user: &str) -> Result<i64, Box<dyn Error>> {
        sqlx::query!(r"INSERT INTO user (name) VALUES (?) ON CONFLICT DO NOTHING", user)
            .execute(db.sql()).await?;
        Ok(sqlx::query!(r"SELECT user_id FROM user WHERE name = ?", user)
            .fetch_one(db.sql()).await?.user_id)
}

impl ServerJob {
    pub async fn ensure(db: &Db, user: &str, name: &str, id: Option<&str>) -> Result<ServerJob, Box<dyn Error>> {
        let id = id.unwrap_or(&slug(name)).to_owned();
        if user.is_empty() || user.contains("/") || user.starts_with(".") { Err(format!("Bad user"))? }
        if id.is_empty()   || id.contains("/")   || id.starts_with(".")   { Err(format!("Bad id"))? }
        let user_id = user_id(db, user).await?;
        sqlx::query!(r"INSERT INTO job (user_id, id, name) VALUES (?, ?, ?) ON CONFLICT DO NOTHING", user_id, id, name)
            .execute(db.sql()).await.map_err(|e| wrap(&e, "Job ensure INSERT"))?;
        let job = sqlx::query!("SELECT job_id FROM job WHERE user_id = ? AND id = ?", user_id, id)
            .fetch_one(db.sql()).await.map_err(|e| wrap(&e, "Job ensure SELECT"))?;

        Ok(ServerJob { db:   db.clone(),
                       user: user.to_string(),
                       id:   id,
                       name: name.to_string(),
                       job_id: job.job_id
        })
    }

    pub async fn new(db: &Db, user: &str, id: &str) -> Result<ServerJob, Box<dyn Error>> {
        if user.is_empty() || user.contains("/") || user.starts_with(".") { Err(format!("Bad user"))? }
        if id.is_empty()   || id.contains("/")   || id.starts_with(".")   { Err(format!("Bad id"))? }
        let job = sqlx::query!(r"SELECT j.job_id, j.name
                                   FROM job j
                                   JOIN user u ON u.user_id = j.user_id
                                  WHERE u.name = ? AND j.id = ?",
                     user, id)
            .fetch_one(db.sql()).await?;
        Ok(ServerJob { db:   db.clone(),
                       user: user.to_string(),
                       id:   id.to_string(),
                       name:  job.name,
                       job_id: job.job_id,
        })
    }

    pub async fn from_id(db: &Db, job_id: i64) -> Result<ServerJob, Box<dyn Error>> {
        let job = sqlx::query!(r"SELECT j.job_id, j.name, u.name as user, j.id
                                   FROM job j
                                   JOIN user u ON u.user_id = j.user_id
                                  WHERE j.job_id = ?", job_id)
            .fetch_one(db.sql()).await?;
        Ok(ServerJob { db:   db.clone(),
                       user: job.user,
                       id:   job.id,
                       name:  job.name,
                       job_id: job.job_id,
        })
    }

    pub async fn jobs(db: &Db) -> Result<Vec<ServerJob>, Box<dyn Error>> {
        Ok(sqlx::query!("SELECT j.job_id, j.id as id, j.name as name, u.name as user FROM job j JOIN user u ON u.user_id = j.user_id")
           .fetch_all(db.sql()).await.map_err(|e| wrap(&e, "get jobs"))?.iter()
           .map(|job|  ServerJob { db: db.clone(),
                                   user: job.user.clone(),
                                   id: job.id.clone(),
                                   name: job.name.clone(),
                                   job_id: job.job_id })
           .collect())
    }

    pub fn job_path(&self)  -> PathBuf {self.db.jobs_path().join(&self.user).join(&self.id)}

    pub async fn runs(&self) -> Result<Vec<ServerRun>, Box<dyn Error>> {
        Ok(sqlx::query!("SELECT r.run_id, r.start, r.end, r.status, r.client_id, r.log FROM run r JOIN job j ON r.job_id = j.job_id WHERE r.job_id = ? ORDER BY r.start DESC", self.job_id)
           .fetch_all(self.db.sql()).await.map_err(|e| wrap(&e, "get runs"))?.iter()
           .map(|run|  ServerRun { job: self.clone(),
                                   date: time_from_timestamp_ms(run.start).into(),
                                   run_id: time_string_from_timestamp_ms(run.start),
                                   run_db_id: run.run_id,
                                   client_id: run.client_id.as_ref().and_then(|id| id.parse::<u128>().ok()),
                                   log_path: run.log.clone().into(), })
           .collect())
    }

    pub async fn latest_run(&self) -> Result<Option<ServerRun>, Box<dyn Error>> {
        // use chrono::TimeZone;
        let run = sqlx::query!("SELECT r.run_id, r.start, r.end, r.status, r.client_id, r.log FROM run r JOIN job j ON r.job_id = j.job_id WHERE r.job_id = ? ORDER BY r.start DESC limit 1", self.job_id)
           .fetch_optional(self.db.sql()).await.map_err(|e| wrap(&e, "get runs"))?;
        Ok(run.map(|run| ServerRun { job: self.clone(),
                                     date: time_from_timestamp_ms(run.start).into(),
                                     run_id: time_string_from_timestamp_ms(run.start),
                                     run_db_id: run.run_id,
                                     client_id: run.client_id.as_ref().and_then(|id| id.parse::<u128>().ok()),
                                     log_path: run.log.clone().into(), }))
    }

    pub async fn run(&self, run_id: &str) -> Result<ServerRun, Box<dyn Error>> {
        ServerRun::from_run_id(self, run_id).await
    }
}

impl ServerRun {
    pub async fn create(sql: &sqlx::SqlitePool, db: PathBuf, user: &str, name:&str, id:Option<&str>, cmd: String, env: Vec<(MaybeUTF8,MaybeUTF8)>) -> Result<ServerRun, Box<dyn Error>> {
        let job = ServerJob::ensure(&Db::new(&sql, &db), user, name, id).await?;
        let env_str = serde_json::to_string(&env)?;
        let date = chrono::Local::now();
        let start = date.timestamp_millis();
        let run_id = date.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let log_path = job.job_path().join(&run_id).join("log");
        let log_str = log_path.as_os_str().to_str().ok_or(format!("bad unicode in {:?}", log_path))?;
        let mut client_id_bytes = [0; 128/8];
        getrandom::getrandom(&mut client_id_bytes)?;
        let client_id: u128 = u128::from_ne_bytes(client_id_bytes);
        let client_id_str = format!("{}", client_id);
        let mut transaction = sql.begin().await?;
        let run_db_id = sqlx::query!("INSERT INTO run (job_id, client_id, cmd, env, log, start) VALUES (?, ?, ?, ?, ?, ?) RETURNING run_id",
                                     job.job_id, client_id_str, cmd, env_str, log_str, start)
            .fetch_one(&mut transaction).await?.run_id;
        transaction.commit().await?;
        let run = ServerRun { run_db_id: run_db_id, job: job, date: date.into(), run_id: run_id, client_id: Some(client_id), log_path: log_path };
        trace!("created {:?}", run.client_id);
        Ok(run)
    }

    pub async fn from_client_id(sql: &sqlx::SqlitePool, db_path: PathBuf, id: u128) -> Result<ServerRun, Box<dyn Error>> {
        let db = Db::new(sql, &db_path);
        let client_id_str = format!("{}",id);
        trace!("looking for {}", client_id_str);
        let run = sqlx::query!("SELECT run_id, job_id, log, start FROM run WHERE client_id = ?", client_id_str)
            .fetch_one(db.sql()).await.map_err(|e| wrap(&e, "Run from_client_id SELECT"))?;
        Ok(ServerRun { job: ServerJob::from_id(&db, run.job_id).await?,
                       run_db_id: run.run_id,
                       date: time_from_timestamp_ms(run.start).into(),
                       run_id: time_string_from_timestamp_ms(run.start),
                       client_id: Some(id),
                       log_path: run.log.clone().into(),
        })
    }
    pub async fn from_run_id(job: &ServerJob, run_id: &str) -> Result<ServerRun, Box<dyn Error>> {
        let start = chrono::DateTime::parse_from_rfc3339(run_id)?;
        let start_timestamp = start.timestamp_millis();
        trace!("looking for {} [{}, {}] in job {}...", run_id, start, start_timestamp, job.job_id);
        let run = sqlx::query!("SELECT run_id, job_id, log, start, client_id FROM run WHERE job_id = ? AND start = ?", job.job_id, start_timestamp)
            .fetch_one(job.db.sql()).await?;
        Ok(ServerRun { job: job.clone(),
                       run_db_id: run.run_id,
                       date: start.into(),
                       run_id: start.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                       client_id: run.client_id.as_ref().and_then(|id| id.parse::<u128>().ok()),
                       log_path: run.log.clone().into(),
        })
    }

    pub fn run_path(&self)             -> PathBuf {self.job.job_path().join(&self.run_id)}

    fn mkdir_p(&self) -> Result<(), Box<dyn Error>> {
        std::fs::DirBuilder::new().recursive(true).create(self.run_path())
            .map_err(|e| wrap(&e, &format!("mkdir -p {}", self.run_path().to_string_lossy())))
    }

    pub async fn get_info(&self) -> Result<ServerRunInfo, Box<dyn Error>> {
        let run = sqlx::query!(r"SELECT cmd, env, end, status FROM run WHERE run_id = ?", self.run_db_id)
            .fetch_one(self.job.db.sql()).await?;
        Ok(ServerRunInfo {
            cmd:    run.cmd,
            env:    serde_json::from_str(&run.env)?,
            end:    run.end.map(|ms| time_from_timestamp_ms(ms).into()),
            status: match run.status { Some(s) => serde_json::from_str(&s)?, _ => None },
        })
    }
    pub async fn info(&self) -> Result<ServerRunInfo, Box<dyn Error>> {
        let mut info = self.get_info().await?;
        if info.status.is_none() {
            let hb = self.heartbeat().await.ok();
            info!("Run [{}] {}/{}/{} is not done. Heartbeat: {:?}", self.run_db_id, self.job.user, self.job.name, self.run_id, hb);
            if let Some(ts) = hb {
                if chrono::Local::now().timestamp_millis() - ts > 30*1000 {
                    info!("Timing out job {} run {} ater {} seconds", self.job.name, self.run_id, (chrono::Local::now().timestamp_millis() - ts)/1000);
                    self.complete(serve::ExitStatus::ServerTimeout).await?;
                    info = self.get_info().await?
                }
            } else { // If we couldn't read the heartbeat file, then write one out right now. This lets us
                     // timeout things that got corrupted or crashed with no heartbeat (or old jobs that
                     // existed before the hearbeat was implemented)
                let x = self.set_heartbeat().await;
                trace!("Set heartbeat: {:?}", x);
            }
        }
        Ok(info)
    }

    pub fn add_stdout(&self, chunk: &str) -> Result<(), Box<dyn Error>> {
        self.mkdir_p().map_err(|e| wrap(&*e, "add_stdout"))?;
        File::options().create(true).append(true).open(&self.log_path).map_err(|e| wrap(&e, &format!("open {}", self.log_path.to_string_lossy())))?
            .write_all(chunk.as_bytes()).map_err(|e| wrap(&e, &format!("write {}", self.log_path.to_string_lossy())))?;
        Ok(())
    }

    pub async fn set_heartbeat(&self) -> Result<(), Box<dyn Error>> {
        let heartbeat = Some(chrono::Local::now().timestamp_millis());
        info!("Run [{}] {}/{}/{} Set heartbeat: {:?}", self.run_db_id, self.job.user, self.job.name, self.run_id, heartbeat);
        sqlx::query!("UPDATE run SET heartbeat = ? WHERE run_id = ?", heartbeat, self.run_db_id).execute(self.job.db.sql()).await?;
        Ok(())
    }

    pub async fn heartbeat(&self) -> Result<i64, Box<dyn Error>> {
        sqlx::query!("SELECT heartbeat FROM run WHERE run_id = ?", self.run_db_id).fetch_one(self.job.db.sql()).await?.heartbeat.ok_or("Missing hearbeat".into())
    }

    pub async fn complete(&self, status: serve::ExitStatus) -> Result<(), Box<dyn Error>> {
        let end = Some(chrono::Local::now().timestamp_millis());
        let status = Some(serde_json::to_string(&status)?);
        trace!("Completing {}/{}/{} with {:?}", self.job.user, self.job.name, self.run_id, status);
        sqlx::query!("UPDATE run SET status = ?, end = ?, client_id = NULL WHERE run_id = ?", status, end, self.run_db_id).execute(self.job.db.sql()).await?;
        Ok(())
    }

    pub fn progress(&self) -> Result<Option<serve::Progress>, Box<dyn Error>> {
        Ok(None)
    }

    pub fn log_len(&self) -> u64 {
        self.log_path.metadata().map(|m| m.len()).unwrap_or(0)
    }

    pub fn log(&self, seek: Option<u64>) -> Result<Option<String>, Box<dyn Error>> {
        use std::io::{Seek,Read};
        if !self.log_path.is_file() { return Ok(None) }
        let mut f = std::fs::File::open(&self.log_path)?;
        if let Some(bytes) = seek {
            f.seek(std::io::SeekFrom::Start(bytes))?;
        }
        let mut buf = vec![];
        f.read_to_end(&mut buf)?;
        Ok(Some(String::from_utf8_lossy(&buf).to_string()))
    }
}

pub fn time_from_timestamp_ms(timestamp_ms: i64) -> chrono::DateTime<chrono::Local> {
    use chrono::TimeZone;
    chrono::Local.timestamp_millis(timestamp_ms).into()
}

pub fn time_string_from_timestamp_ms(timestamp_ms: i64) -> String {
    time_from_timestamp_ms(timestamp_ms).to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_db() -> (sqlx::SqlitePool, tempfile::TempDir) {
        let db_path = tempfile::Builder::new().prefix("syncron-test").tempdir().unwrap();
        let sql = sqlx::SqlitePool::connect(&format!("{}/{}?mode=rwc", &db_path.path().to_string_lossy(), "syncron.sqlite3")).await.unwrap();
        crate::db::MIGRATOR.run(&sql).await.expect("migrate");
        (sql, db_path)
    }

    fn test_dir() -> tempfile::TempDir {
        tempfile::Builder::new().prefix("syncron-test").tempdir().unwrap()
    }

    impl PartialEq for ServerJob {
        fn eq(&self, a: &ServerJob) -> bool {
            self.user    == a.user   &&
            self.id      == a.id     &&
            self.name    == a.name   &&
            self.job_id  == a.job_id
        }
    }

    impl PartialEq for ServerRun {
        fn eq(&self, a: &ServerRun) -> bool {
            self.job        == a.job       &&
            self.date       == a.date      &&
            self.run_id     == a.run_id    &&
            self.run_db_id  == a.run_db_id &&
            self.client_id  == a.client_id &&
            self.log_path   == a.log_path
        }
    }

    macro_rules! assert_file_eq {
        ( $path:expr, $contents:expr ) => {
            let b = std::fs::read($path).expect(&$path.to_string_lossy());
            assert_eq!(String::from_utf8_lossy(&b), $contents);
        }
    }

    #[test]
    fn init_logging() {
        simple_logger::SimpleLogger::new().init().unwrap();
    }

    #[tokio::test]
    async fn basic() {
        let (sql, db) = test_db().await;
        let cmd = "echo a simple test";
        let env = vec![
            (MaybeUTF8::new(OsString::from("PATH")),            MaybeUTF8::new(OsString::from("Something:something_else/"))),
            (MaybeUTF8::new(OsString::from("HOME")),            MaybeUTF8::new(OsString::from("/home/my-home-dir"))),
            (MaybeUTF8::new(OsString::from("SOMETHING WACKY")), MaybeUTF8::new(OsString::from("Oh\nDear"))),
        ];
        let run = ServerRun::create(&sql, db.path().into(), "test-user", "David's The _absolute_ Greatest", None, cmd.to_string(), env).await.expect("ServerRun create worked");
        assert_eq!(run.job.id, "david-s-the-absolute-greatest");

        let id = run.client_id.expect("got client_id");
        let mut run2 = ServerRun::from_client_id(&sql, db.path().into(), id).await.expect("ServerRun::from_client_id()");
        assert!(run.date - run2.date < chrono::Duration::milliseconds(1000), "Dates are less close than expected {} vs {}", run.date, run2.date);
        run2.date = run.date;
        assert_eq!(run, run2);
        run2.add_stdout("Some text. ").expect("text added");
        run2.add_stdout("Some more text.\n").expect("more text added");
        run2.add_stdout("Even more text.\n").expect("even more text added");
        run2.complete(serve::ExitStatus::Exited(0)).await.expect("completed with no errors");

        assert_file_eq!(&db.path().join("jobs").join("test-user").join("david-s-the-absolute-greatest").join(&run.run_id).join("log"), "Some text. Some more text.\nEven more text.\n");

        let run3 = ServerRun::from_client_id(&sql, db.path().into(), id).await;
        assert!(run3.is_err(), "ServerRun::from_client_id() returned was {:?}", run3);
    }

    #[tokio::test]
    async fn heartbeat_timeout() {
        let (sql, db) = test_db().await;
        let cmd = "echo a simple test";
        let run = ServerRun::create(&sql, db.path().into(), "test-user", "David's The _absolute_ Greatest", None, cmd.to_string(), vec![]).await.expect("ServerRun create worked");

        sqlx::query!("UPDATE run SET heartbeat = 0 WHERE run_id = ?", run.run_db_id).execute(&sql).await.expect("update run set heartbeat");
        let info = run.info().await.expect("got info");
        assert_eq!(info.status, Some(serve::ExitStatus::ServerTimeout));
    }

    #[tokio::test]
    async fn integration() {
        let (sql, db) = test_db().await;
        let cmd = "echo a simple test";
        std::env::set_var("MY_ENV_VAR", "some value");
        let db_path = db.path().to_path_buf();
        let _serve = tokio::spawn(async move { serve::serve(32923, db_path, true).await.unwrap(); });
        let _client = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await; // HACK
            let job = crate::job::ClientJob::new("http://127.0.0.1:32923/".parse().unwrap(), "test-user", "My Job", Some("my-id"), None, cmd).await.unwrap();
            let log_path = sqlx::query!("SELECT log FROM run WHERE client_id = ?", job.id).fetch_one(&sql).await.expect("SELECT log FROM run").log;
            job.run().await.expect("job ran");
            assert_file_eq!(&db.path().join(&log_path), "a simple test\n");

            let jobs: Vec<serve::JobInfo> = serde_json::from_str(&job.api.get("/jobs").await.expect("GET /jobs")).expect("GET /jobs parse");
            assert_eq!(jobs.len(),   1);
            assert_eq!(jobs[0].id,   "my-id");
            assert_eq!(jobs[0].user, "test-user");
            assert_eq!(jobs[0].name, "My Job");

            let runs: Vec<serve::RunInfo> = serde_json::from_str(&job.api.get(&jobs[0].runs_url).await.expect("GET runs")).expect("GET runs parse");
            assert_eq!(runs.len(), 1);
            println!("runs: {:?}", runs);
            assert_eq!(runs[0].status, Some(serve::ExitStatus::Exited(0)));

            let run: serve::RunInfoFull = serde_json::from_str(&job.api.get(&runs[0].url.as_ref().expect("runs[0].url")).await.expect("GET run")).expect("GET run parse");
            assert_eq!(run.cmd, cmd);
            assert!(run.env.contains(&(MaybeUTF8::new(OsString::from("MY_ENV_VAR")), MaybeUTF8::new(OsString::from("some value")))));
            assert_eq!(run.log.expect("run.log"), "a simple test\n");

            job.api.post("/shutdown", &[]).await.expect("POST /shutdown");
        }).await.unwrap();
        _serve.await.unwrap();
    }

    #[tokio::test]
    async fn timeout() {
        trace!("Testing");
        let (sql, db) = test_db().await;
        let cmd = "sleep 10";
        let db_path = db.path().to_path_buf();
        let _serve = tokio::spawn(async move { serve::serve(32924, db_path, true).await.unwrap(); });
        let _client = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await; // HACK
            let job = crate::job::ClientJob::new("http://127.0.0.1:32924/".parse().unwrap(), "test-user", "My Bad Job", None, Some(std::time::Duration::from_millis(1500)), cmd).await.unwrap();
            let log_path = sqlx::query!("SELECT log FROM run WHERE client_id = ?", job.id).fetch_one(&sql).await.expect("SELECT log FROM run").log;
            job.run().await.expect("job ran");
            assert_eq!(db.path().join(&log_path).exists(), false);

            job.api.post("/shutdown", &[]).await.expect("POST /shutdown");
        }).await.unwrap();
        _serve.await.unwrap();
    }
}
