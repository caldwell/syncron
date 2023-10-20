// Copyright © 2022 David Caldwell <david@porkrind.org>

use std::path::{Path,PathBuf};

use sqlx::migrate::Migrator;
pub static MIGRATOR: Migrator = sqlx::migrate!(); // defaults to "./migrations"

#[derive(Debug, Clone)]
pub struct Db {
    db_path: PathBuf,
    sql: sqlx::SqlitePool,
}

impl Db {
    pub async fn new(db_path: &Path) -> Result<Db, Box<dyn Error>> {
        let pool = sqlx::pool::PoolOptions::new()
            .acquire_timeout(std::time::Duration::from_secs(5))
            .max_connections(500)
            .idle_timeout(Some(std::time::Duration::from_secs(5*60)))
            .connect_with(sqlx::sqlite::SqliteConnectOptions::new()
                          .filename(db_path.join("syncron.sqlite3"))
                          .create_if_missing(true)
                          .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal) // Should be the default but lets be explicit
                          .synchronous(sqlx::sqlite::SqliteSynchronous::Normal)).await?; // Dont constantly sync(). Makes writes faster on a shared disk.
        let db = Db{ db_path: db_path.into(),
                     sql: pool, };
        db.migrate().await?;
        Ok(db)
    }

    pub fn sql(&self)               -> &sqlx::SqlitePool { &self.sql }
    pub fn jobs_path(&self)         -> PathBuf { "jobs".into() }
    pub async fn migrate(&self)     -> Result<(), Box<dyn Error>> {
        MIGRATOR.run(&self.sql).await.map_err(|e| wrap(&e, "Failed to initialize SQLx database"))
    }
}


use std::error::Error;
use std::fs::File;
use std::io::Write;

use crate::serve;
use crate::maybe_utf8::MaybeUTF8;
use crate::wrap;

#[derive(Debug, Clone)]
pub struct Job {
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
pub struct Run {
    pub job: Job,
    pub date: chrono::DateTime<chrono::Local>,
    pub run_id: String,
    pub run_db_id: i64,
    pub client_id: Option<u128>,
    pub log_path: PathBuf, // Relative to db directory. use log_path() to get read actual file path
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct RunInfo {
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

impl Job {
    #[tracing::instrument(skip(db),ret)]
    pub async fn ensure(db: &Db, user: &str, name: &str, id: Option<&str>) -> Result<Job, Box<dyn Error>> {
        let id = id.unwrap_or(&slug(name)).to_owned();
        if user.is_empty() || user.contains("/") || user.starts_with(".") { Err(format!("Bad user"))? }
        if id.is_empty()   || id.contains("/")   || id.starts_with(".")   { Err(format!("Bad id"))? }
        let user_id = user_id(db, user).await?;
        sqlx::query!(r"INSERT INTO job (user_id, id, name) VALUES (?, ?, ?) ON CONFLICT DO NOTHING", user_id, id, name)
            .execute(db.sql()).await.map_err(|e| wrap(&e, "Job ensure INSERT"))?;
        let job = sqlx::query!("SELECT job_id FROM job WHERE user_id = ? AND id = ?", user_id, id)
            .fetch_one(db.sql()).await.map_err(|e| wrap(&e, "Job ensure SELECT"))?;

        Ok(Job { db:   db.clone(),
                       user: user.to_string(),
                       id:   id,
                       name: name.to_string(),
                       job_id: job.job_id
        })
    }

    pub async fn new(db: &Db, user: &str, id: &str) -> Result<Job, Box<dyn Error>> {
        if user.is_empty() || user.contains("/") || user.starts_with(".") { Err(format!("Bad user"))? }
        if id.is_empty()   || id.contains("/")   || id.starts_with(".")   { Err(format!("Bad id"))? }
        let job = sqlx::query!(r"SELECT j.job_id, j.name
                                   FROM job j
                                   JOIN user u ON u.user_id = j.user_id
                                  WHERE u.name = ? AND j.id = ?",
                     user, id)
            .fetch_one(db.sql()).await?;
        Ok(Job { db:   db.clone(),
                       user: user.to_string(),
                       id:   id.to_string(),
                       name:  job.name,
                       job_id: job.job_id,
        })
    }

    pub async fn from_id(db: &Db, job_id: i64) -> Result<Job, Box<dyn Error>> {
        let job = sqlx::query!(r"SELECT j.job_id, j.name, u.name as user, j.id
                                   FROM job j
                                   JOIN user u ON u.user_id = j.user_id
                                  WHERE j.job_id = ?", job_id)
            .fetch_one(db.sql()).await?;
        Ok(Job { db:   db.clone(),
                       user: job.user,
                       id:   job.id,
                       name:  job.name,
                       job_id: job.job_id,
        })
    }

    pub async fn jobs(db: &Db) -> Result<Vec<Job>, Box<dyn Error>> {
        Ok(sqlx::query!("SELECT j.job_id, j.id as id, j.name as name, u.name as user FROM job j JOIN user u ON u.user_id = j.user_id")
           .fetch_all(db.sql()).await.map_err(|e| wrap(&e, "get jobs"))?.iter()
           .map(|job|  Job { db: db.clone(),
                                   user: job.user.clone(),
                                   id: job.id.clone(),
                                   name: job.name.clone(),
                                   job_id: job.job_id })
           .collect())
    }

    pub fn job_path(&self)  -> PathBuf {self.db.jobs_path().join(&self.user).join(&self.id)}
    pub fn run_path(&self, run_id: &str) -> PathBuf {self.job_path().join(&run_id)}

    pub async fn runs(&self, num: Option<u32>, before: Option<u64>, after:Option<u64>) -> Result<Vec<Run>, Box<dyn Error>> {
        let (num, before, after) = (num.unwrap_or(u32::MAX), before.map(|n| n as i64).unwrap_or(i64::MAX), after.map(|n| n as i64).unwrap_or(0i64));
        Ok(sqlx::query!("SELECT r.run_id, r.start, r.end, r.status, r.client_id, r.log FROM run r JOIN job j ON r.job_id = j.job_id WHERE r.job_id = ? AND r.start > ? AND r.start < ? ORDER BY r.start DESC LIMIT ?",
                        self.job_id, after, before, num)
           .fetch_all(self.db.sql()).await.map_err(|e| wrap(&e, "get runs"))?.iter()
           .map(|run|  Run { job: self.clone(),
                                   date: time_from_timestamp_ms(run.start).into(),
                                   run_id: time_string_from_timestamp_ms(run.start),
                                   run_db_id: run.run_id,
                                   client_id: run.client_id.as_ref().and_then(|id| id.parse::<u128>().ok()),
                                   log_path: run.log.clone().into(), })
           .collect())
    }

    pub async fn runs_from_ids(&self, ids: &[&str]) -> Result<Vec<Run>, Box<dyn Error>> {
        let ids = ids.iter().map(|id| -> Result<i64, Box<dyn Error>> {
            let start = chrono::DateTime::parse_from_rfc3339(id)?;
            Ok(start.timestamp_millis())
        }).collect::<Result<Vec<i64>, Box<dyn Error>>>()?;

        let id_list = ids.iter().map(|id| id.to_string()).intersperse(",".to_string()).collect::<String>();
        #[derive(sqlx::FromRow)]
        struct Row { run_id: i64, start: i64, client_id: Option<String>, log: String }
        Ok(sqlx::query_as::<_, Row>(&format!("SELECT r.run_id, r.start, r.client_id, r.log FROM run r JOIN job j ON r.job_id = j.job_id WHERE r.job_id = ? AND r.start IN ({}) ORDER BY r.start", id_list))
           .bind(self.job_id)
           .fetch_all(self.db.sql()).await.map_err(|e| wrap(&e, "get runs"))?.iter()
           .map(|run|  Run { job: self.clone(),
                             date: time_from_timestamp_ms(run.start).into(),
                             run_id: time_string_from_timestamp_ms(run.start),
                             run_db_id: run.run_id,
                             client_id: run.client_id.as_ref().and_then(|id| id.parse::<u128>().ok()),
                             log_path: run.log.clone().into(), })
           .collect())
    }

    pub async fn latest_run(&self) -> Result<Option<Run>, Box<dyn Error>> {
        let run = sqlx::query!("SELECT r.run_id, r.start, r.end, r.status, r.client_id, r.log FROM run r JOIN job j ON r.job_id = j.job_id WHERE r.job_id = ? ORDER BY r.start DESC limit 1", self.job_id)
           .fetch_optional(self.db.sql()).await.map_err(|e| wrap(&e, "get runs"))?;
        Ok(run.map(|run| Run { job: self.clone(),
                                     date: time_from_timestamp_ms(run.start).into(),
                                     run_id: time_string_from_timestamp_ms(run.start),
                                     run_db_id: run.run_id,
                                     client_id: run.client_id.as_ref().and_then(|id| id.parse::<u128>().ok()),
                                     log_path: run.log.clone().into(), }))
    }

    pub async fn run(&self, run_id: &str) -> Result<Run, Box<dyn Error>> {
        Run::from_run_id(self, run_id).await
    }

    pub async fn successes(&self, before: Option<u64>, after:Option<u64>) -> Result<Vec<(i64, Option<bool>)>, Box<dyn Error>> {
        let (before, after) = (before.map(|n| n as i64).unwrap_or(i64::MAX), after.map(|n| n as i64).unwrap_or(0i64));
        Ok(sqlx::query!("SELECT start, success FROM run WHERE job_id = ? AND start > ? AND start < ? ORDER BY start",
                        self.job_id, after, before)
           .fetch_all(self.db.sql()).await.map_err(|e| wrap(&e, "get hist"))?.iter()
           .map(|run| (run.start, run.success.map(|s| s != 0)))
           .collect())
    }
}

impl Run {
    pub async fn create(db: &Db, user: &str, name:&str, id:Option<&str>, cmd: String, env: Vec<(MaybeUTF8,MaybeUTF8)>) -> Result<Run, Box<dyn Error>> {
        let job = Job::ensure(db, user, name, id).await?;
        let env_str = serde_json::to_string(&env)?;
        let date = chrono::Local::now();
        let start = date.timestamp_millis();
        let run_id = date.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let log_path = job.run_path(&run_id).join("log");
        let log_str = log_path.as_os_str().to_str().ok_or(format!("bad unicode in {:?}", log_path))?;
        let mut client_id_bytes = [0; 128/8];
        getrandom::getrandom(&mut client_id_bytes)?;
        let client_id: u128 = u128::from_ne_bytes(client_id_bytes);
        let client_id_str = format!("{}", client_id);
        let mut transaction = db.sql().begin().await?;
        let run_db_id = sqlx::query!("INSERT INTO run (job_id, client_id, cmd, env, log, start) VALUES (?, ?, ?, ?, ?, ?) RETURNING run_id",
                                     job.job_id, client_id_str, cmd, env_str, log_str, start)
            .fetch_one(&mut transaction).await?.run_id;
        transaction.commit().await?;
        let run = Run { run_db_id: run_db_id, job: job, date: date.into(), run_id: run_id, client_id: Some(client_id), log_path: log_path };
        trace!("created {:?}", run.client_id);
        Ok(run)
    }

    #[tracing::instrument(skip(db),ret)]
    pub async fn from_client_id(db: &Db, id: u128) -> Result<Run, Box<dyn Error>> {
        let client_id_str = format!("{}",id);
        trace!("looking for {}", client_id_str);
        let run = sqlx::query!("SELECT run_id, job_id, log, start FROM run WHERE client_id = ?", client_id_str)
            .fetch_one(db.sql()).await.map_err(|e| wrap(&e, "Run from_client_id SELECT"))?;
        Ok(Run { job: Job::from_id(&db, run.job_id).await?,
                       run_db_id: run.run_id,
                       date: time_from_timestamp_ms(run.start).into(),
                       run_id: time_string_from_timestamp_ms(run.start),
                       client_id: Some(id),
                       log_path: run.log.clone().into(),
        })
    }
    pub async fn from_run_id(job: &Job, run_id: &str) -> Result<Run, Box<dyn Error>> {
        let start = chrono::DateTime::parse_from_rfc3339(run_id)?;
        let start_timestamp = start.timestamp_millis();
        trace!("looking for {} [{}, {}] in job {}...", run_id, start, start_timestamp, job.job_id);
        let run = sqlx::query!("SELECT run_id, job_id, log, start, client_id FROM run WHERE job_id = ? AND start = ?", job.job_id, start_timestamp)
            .fetch_one(job.db.sql()).await?;
        Ok(Run { job: job.clone(),
                       run_db_id: run.run_id,
                       date: start.into(),
                       run_id: start.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                       client_id: run.client_id.as_ref().and_then(|id| id.parse::<u128>().ok()),
                       log_path: run.log.clone().into(),
        })
    }

    pub fn log_path(&self)             -> PathBuf {self.job.db.db_path.join(&self.log_path)} // Full path from cwd to log
    pub fn run_path(&self)             -> PathBuf {self.job.run_path(&self.run_id)}          // Relative path from db to run dir

    fn mkdir_p(&self) -> Result<(), Box<dyn Error>> {
        std::fs::DirBuilder::new().recursive(true).create(self.job.db.db_path.join(self.run_path()))
            .map_err(|e| wrap(&e, &format!("mkdir -p {}", self.job.db.db_path.join(self.run_path()).to_string_lossy())))
    }

    pub async fn get_info(&self) -> Result<RunInfo, Box<dyn Error>> {
        let run = sqlx::query!(r"SELECT cmd, env, end, status FROM run WHERE run_id = ?", self.run_db_id)
            .fetch_one(self.job.db.sql()).await?;
        Ok(RunInfo {
            cmd:    run.cmd,
            env:    serde_json::from_str(&run.env)?,
            end:    run.end.map(|ms| time_from_timestamp_ms(ms).into()),
            status: match run.status { Some(s) => serde_json::from_str(&s)?, _ => None },
        })
    }
    pub async fn info(&self) -> Result<RunInfo, Box<dyn Error>> {
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
        File::options().create(true).append(true).open(&self.log_path()).map_err(|e| wrap(&e, &format!("open {}", self.log_path().to_string_lossy())))?
            .write_all(chunk.as_bytes()).map_err(|e| wrap(&e, &format!("write {}", self.log_path().to_string_lossy())))?;
        Ok(())
    }

    #[tracing::instrument(skip(self),ret)]
    pub async fn set_heartbeat(&self) -> Result<(), Box<dyn Error>> {
        let heartbeat = Some(chrono::Local::now().timestamp_millis());
        info!("Run [{}] {}/{}/{} Set heartbeat: {:?}", self.run_db_id, self.job.user, self.job.name, self.run_id, heartbeat);
        sqlx::query!("UPDATE run SET heartbeat = ? WHERE run_id = ?", heartbeat, self.run_db_id).execute(self.job.db.sql()).await?;
        Ok(())
    }

    pub async fn heartbeat(&self) -> Result<i64, Box<dyn Error>> {
        sqlx::query!("SELECT heartbeat FROM run WHERE run_id = ?", self.run_db_id).fetch_one(self.job.db.sql()).await?.heartbeat.ok_or("Missing hearbeat".into())
    }

    #[tracing::instrument(skip(self),ret)]
    pub async fn complete(&self, status: serve::ExitStatus) -> Result<(), Box<dyn Error>> {
        let end = Some(chrono::Local::now().timestamp_millis());
        let status_json = Some(serde_json::to_string(&status)?);
        let success = match status {
            serve::ExitStatus::Exited(0) => true,
            // If it didn't print anything but stil exited with non-zero status, then consider it success. This doesn't
            // seem strictly correct, but cron doesn't care about exit status and so a lot of cron jobs return false
            // (especially conditional ones).
            serve::ExitStatus::Exited(_) if self.log_len() == 0 => true,
            _ => false,
        };
        trace!("Completing {}/{}/{} with {:?}", self.job.user, self.job.name, self.run_id, status);
        sqlx::query!("UPDATE run SET status = ?, success = ?, end = ?, client_id = NULL WHERE run_id = ?", status_json, success, end, self.run_db_id).execute(self.job.db.sql()).await?;
        Ok(())
    }

    pub fn progress(&self) -> Result<Option<serve::Progress>, Box<dyn Error>> {
        Ok(None)
    }

    pub fn log_len(&self) -> u64 {
        self.log_path().metadata().map(|m| m.len()).unwrap_or(0)
    }

    pub fn log(&self, seek: Option<u64>) -> Result<Option<(String, u64)>, Box<dyn Error>> {
        use std::io::{Seek,Read};
        if !self.log_path().is_file() { return Ok(None) }
        let mut f = std::fs::File::open(&self.log_path())?;
        if let Some(bytes) = seek {
            f.seek(std::io::SeekFrom::Start(bytes))?;
        }
        let mut buf = vec![];
        f.read_to_end(&mut buf)?;
        Ok(Some((String::from_utf8_lossy(&buf).to_string(), seek.unwrap_or(0) + buf.len() as u64)))
    }
}

pub fn time_from_timestamp_ms(timestamp_ms: i64) -> chrono::DateTime<chrono::Local> {
    use chrono::TimeZone;
    chrono::Local.timestamp_millis(timestamp_ms).into()
}

pub fn time_string_from_timestamp_ms(timestamp_ms: i64) -> String {
    time_from_timestamp_ms(timestamp_ms).to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

