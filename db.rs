// Copyright © 2022 David Caldwell <david@porkrind.org>

use std::path::{Path,PathBuf};

use chrono::Datelike;
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
    pub last_progress_json: Option<String>,
    pub settings: JobSettings,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct JobSettings {
    #[serde(default)]
    pub retention: JobRetention,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum JobRetention {
    #[default]
    Default,
    #[serde(untagged)]
    Custom(RetentionSettings),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default, Copy, PartialEq)]
pub struct RetentionSettings {
    pub max_age:  Option<usize>,
    pub max_runs: Option<usize>,
    pub max_size: Option<usize>,
}

#[derive(Debug)]
pub struct Run {
    pub job: Job,
    pub date: chrono::DateTime<chrono::Local>,
    pub duration_ms: Option<u64>,
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

// progress files in the run dir
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug)]
struct ProgressChunk {
    timestamp_ms: i64,
    bytes: usize,
}

// Array of these in the job db row
#[derive(Clone, Copy, Default, Debug, serde::Serialize, serde::Deserialize)]
pub struct ProgressStat {
    pub dt_pct: f64,
    pub bytes_pct: f64,
    pub timestamp_ms: i64,
    pub bytes: usize,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Pruned {
    pub job_id: i64,
    pub run_id: String,
    pub size: usize,
    pub reason: String,
}

#[derive(Copy, Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct RunCount {
    pub runs: usize,
    pub size: usize,
}

#[derive(Copy, Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct PruneStats {
    pub pruned: RunCount,
    pub kept: RunCount,
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
        let job = sqlx::query!("SELECT job_id, last_progress, settings FROM job WHERE user_id = ? AND id = ?", user_id, id)
            .fetch_one(db.sql()).await.map_err(|e| wrap(&e, "Job ensure SELECT"))?;

        Ok(Job { db:   db.clone(),
                 user: user.to_string(),
                 id:   id,
                 name: name.to_string(),
                 job_id: job.job_id,
                 last_progress_json: job.last_progress,
                 settings: serde_sqlite_jsonb::from_reader(&*job.settings).unwrap_or(JobSettings::default()),
        })
    }

    pub async fn new(db: &Db, user: &str, id: &str) -> Result<Job, Box<dyn Error>> {
        if user.is_empty() || user.contains("/") || user.starts_with(".") { Err(format!("Bad user"))? }
        if id.is_empty()   || id.contains("/")   || id.starts_with(".")   { Err(format!("Bad id"))? }
        let job = sqlx::query!(r"SELECT j.job_id, j.name, j.last_progress, j.settings as settings
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
                 last_progress_json: job.last_progress,
                 settings: serde_sqlite_jsonb::from_reader(&*job.settings).unwrap_or(JobSettings::default()),
        })
    }

    pub async fn from_id(db: &Db, job_id: i64) -> Result<Job, Box<dyn Error>> {
        let job = sqlx::query!(r"SELECT j.job_id, j.name, u.name as user, j.id, j.last_progress, j.settings
                                   FROM job j
                                   JOIN user u ON u.user_id = j.user_id
                                  WHERE j.job_id = ?", job_id)
            .fetch_one(db.sql()).await?;
        Ok(Job { db:   db.clone(),
                 user: job.user,
                 id:   job.id,
                 name:  job.name,
                 job_id: job.job_id,
                 last_progress_json: job.last_progress,
                 settings: serde_sqlite_jsonb::from_reader(&*job.settings).unwrap_or(JobSettings::default()),
        })
    }

    pub async fn jobs(db: &Db) -> Result<Vec<Job>, Box<dyn Error>> {
        Ok(sqlx::query!("SELECT j.job_id, j.id as id, j.name as name, u.name as user, j.last_progress, j.settings FROM job j JOIN user u ON u.user_id = j.user_id")
           .fetch_all(db.sql()).await.map_err(|e| wrap(&e, "get jobs"))?.iter()
           .map(|job|  Job { db: db.clone(),
                             user: job.user.clone(),
                             id: job.id.clone(),
                             name: job.name.clone(),
                             job_id: job.job_id,
                             last_progress_json: job.last_progress.clone(),
                             settings: serde_sqlite_jsonb::from_reader(&*job.settings).unwrap_or(JobSettings::default()),
           })
           .collect())
    }

    pub fn job_path(&self)  -> PathBuf {self.db.jobs_path().join(&self.user).join(&self.id)}
    pub fn run_path(&self, date: chrono::DateTime<chrono::Local>) -> PathBuf {
        self.job_path().join(date.year().to_string()).join(date.month().to_string()).join(date.day().to_string()).join(date.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
    }

    pub async fn runs(&self, num: Option<u32>, before: Option<u64>, after:Option<u64>) -> Result<Vec<Run>, Box<dyn Error>> {
        let (num, before, after) = (num.unwrap_or(u32::MAX), before.map(|n| n as i64).unwrap_or(i64::MAX), after.map(|n| n as i64).unwrap_or(0i64));
        Ok(sqlx::query!("SELECT r.run_id, r.start, r.end, r.status, r.client_id, r.log FROM run r JOIN job j ON r.job_id = j.job_id WHERE r.job_id = ? AND r.start > ? AND r.start < ? ORDER BY r.start DESC LIMIT ?",
                        self.job_id, after, before, num)
           .fetch_all(self.db.sql()).await.map_err(|e| wrap(&e, "get runs"))?.iter()
           .map(|run|  Run { job: self.clone(),
                             date: time_from_timestamp_ms(run.start).into(),
                             duration_ms: run.end.and_then(|e| ((e as u64).checked_sub(run.start as u64))),
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
        struct Row { run_id: i64, start: i64, end: Option<i64>, client_id: Option<String>, log: String }
        Ok(sqlx::query_as::<_, Row>(&format!("SELECT r.run_id, r.start, r.end, r.client_id, r.log FROM run r JOIN job j ON r.job_id = j.job_id WHERE r.job_id = ? AND r.start IN ({}) ORDER BY r.start", id_list))
           .bind(self.job_id)
           .fetch_all(self.db.sql()).await.map_err(|e| wrap(&e, "get runs"))?.iter()
           .map(|run|  Run { job: self.clone(),
                             date: time_from_timestamp_ms(run.start).into(),
                             duration_ms: run.end.and_then(|e| ((e as u64).checked_sub(run.start as u64))),
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
                               duration_ms: run.end.and_then(|e| ((e as u64).checked_sub(run.start as u64))),
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

    pub fn last_progress(&self) -> Result<Option<Vec<ProgressStat>>, Box<dyn Error>> { // deserialize this lazily. We only need it sometimes.
        Ok(match self.last_progress_json {
            None => None,
            Some(ref s) => Some(serde_json::from_str(s).map_err(|e| wrap(&e, &format!("last_progress column corrupt for job {}", self.name)))?),
        })
    }

    pub async fn update_settings(&self, new_settings: &JobSettings) -> Result<(), Box<dyn Error>> {
        let json = serde_json::to_string(&new_settings)?;
        sqlx::query!("UPDATE job SET settings = jsonb(?) WHERE job_id = ?", json, self.job_id).execute(self.db.sql()).await?;
        Ok(())
    }

    async fn _prune(&self, dry_run: bool, mut stats: Option<&mut PruneStats>, settings: Option<RetentionSettings>) -> Result<Vec<Pruned>, Box<dyn Error>> {
        let retention = settings.unwrap_or(match self.settings.retention {
            JobRetention::Custom(retention) => retention,
            JobRetention::Default => Settings::load(&self.db).await?.retention,
        });
        debug!("Retention settings for {}: {:?}", self.name, retention);
        if retention == RetentionSettings::default() && stats.is_none() { return Ok(vec![]) }
        let runs = self.runs(None, None, None).await?;
        debug!("Considering {} [{} runs]", self.name, runs.len());
        let mut total = 0;
        let sizes: Vec<(usize,usize)> = runs.iter().map(|r| { let size = r.log_len() as usize; total += size; (size, total) }).collect();
        let now = chrono::Local::now();
        let mut pruned = vec![];
        if let Some(ref mut stats) = stats { **stats = PruneStats::default() };
        for (n, (run, (size, total_size))) in runs.iter().zip(sizes.iter()).enumerate().rev() {
            let (reason, will_prune) = match (retention.max_age.map(|t| t as i64), now.signed_duration_since(run.date).num_days(),
                                              retention.max_runs,
                                              retention.max_size, *total_size) {
                (Some(max), age, _,         _,         _)    if age  >= max => (format!("exceeded max age  ({age} > {max})",  ), true),
                (_,         _,   Some(max), _,         _)    if n    >= max => (format!("exceeded max runs ({n} > {max})",    ), true),
                (_,         _,   _,         Some(max), size) if size >= max => (format!("exceeded max size ({size} > {max})", ), true),
                _ => (format!(""), false),
            };
            if will_prune {
                debug!("Pruning {}/{}: {}", self.name, run.run_id, reason);
                if let Err(e) = if dry_run { Ok(()) } else { run.delete().await } {
                    warn!("Couldn't delete {}/{}: {}", self.name, run.run_id, e);
                } else {
                    if pruned.len() < 1000 { // with millions pruned I started running out of system RAM (64G) due to this list. Having too many in the list isn't even useful for the UI, so lets just cap this for sanity.
                        pruned.push(Pruned { job_id: self.job_id, run_id: run.run_id.clone(), size: *size, reason });
                    }
                    if let Some(ref mut stats) = stats {
                        stats.pruned.runs += 1;
                        stats.pruned.size += size;
                    }
                }
            } else {
                debug!("Not Pruning {}/{}: {:?},{:?} {:?},{:?} {:?},{:?}", self.name, run.run_id, retention.max_age, now.signed_duration_since(run.date).num_days(), retention.max_runs, n, retention.max_size, total_size);
                if let Some(ref mut stats) = stats {
                    stats.kept.runs += 1;
                    stats.kept.size += size;
                }
            }
        }
        Ok(pruned)
    }
    pub async fn prune_dry_run(&self, stats: Option<&mut PruneStats>, settings: Option<RetentionSettings>) -> Result<Vec<Pruned>, Box<dyn Error>> { self._prune(true,  stats, settings).await }
    pub async fn prune(&self,         stats: Option<&mut PruneStats>) -> Result<Vec<Pruned>, Box<dyn Error>> { self._prune(false, stats, None).await }
}

impl Run {
    pub async fn create(db: &Db, user: &str, name:&str, id:Option<&str>, cmd: String, env: Vec<(MaybeUTF8,MaybeUTF8)>) -> Result<Run, Box<dyn Error>> {
        let job = Job::ensure(db, user, name, id).await?;
        let env_str = serde_json::to_string(&env)?;
        let date = chrono::Local::now();
        let start = date.timestamp_millis();
        let run_id = date.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let log_path = job.run_path(date).join("log");
        let log_str = log_path.as_os_str().to_str().ok_or(format!("bad unicode in {:?}", log_path))?;
        let mut client_id_bytes = [0; 128/8];
        getrandom::getrandom(&mut client_id_bytes)?;
        let client_id: u128 = u128::from_ne_bytes(client_id_bytes);
        let client_id_str = format!("{}", client_id);
        let mut transaction = db.sql().begin().await?;
        let run_db_id = sqlx::query!("INSERT INTO run (job_id, client_id, cmd, env, log, start) VALUES (?, ?, ?, ?, ?, ?) RETURNING run_id",
                                     job.job_id, client_id_str, cmd, env_str, log_str, start)
            .fetch_one(&mut *transaction).await?.run_id;
        transaction.commit().await?;
        let run = Run { run_db_id: run_db_id, job: job, date: date.into(), duration_ms: None, run_id: run_id, client_id: Some(client_id), log_path: log_path };
        trace!("created {:?}", run.client_id);
        Ok(run)
    }

    #[tracing::instrument(skip(db),ret)]
    pub async fn from_client_id(db: &Db, id: u128) -> Result<Run, Box<dyn Error>> {
        let client_id_str = format!("{}",id);
        trace!("looking for {}", client_id_str);
        let run = sqlx::query!("SELECT run_id, job_id, log, start, end FROM run WHERE client_id = ?", client_id_str)
            .fetch_one(db.sql()).await.map_err(|e| wrap(&e, "Run from_client_id SELECT"))?;
        Ok(Run { job: Job::from_id(&db, run.job_id).await?,
                 run_db_id: run.run_id,
                 date: time_from_timestamp_ms(run.start).into(),
                 duration_ms: run.end.and_then(|e| ((e as u64).checked_sub(run.start as u64))),
                 run_id: time_string_from_timestamp_ms(run.start),
                 client_id: Some(id),
                 log_path: run.log.clone().into(),
        })
    }
    pub async fn from_run_id(job: &Job, run_id: &str) -> Result<Run, Box<dyn Error>> {
        let start = chrono::DateTime::parse_from_rfc3339(run_id)?;
        let start_timestamp = start.timestamp_millis();
        trace!("looking for {} [{}, {}] in job {}...", run_id, start, start_timestamp, job.job_id);
        let run = sqlx::query!("SELECT run_id, job_id, log, start, end, client_id FROM run WHERE job_id = ? AND start = ?", job.job_id, start_timestamp)
            .fetch_one(job.db.sql()).await?;
        Ok(Run { job: job.clone(),
                       run_db_id: run.run_id,
                       date: start.into(),
                       duration_ms: run.end.and_then(|e| ((e as u64).checked_sub(run.start as u64))),
                       run_id: start.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                       client_id: run.client_id.as_ref().and_then(|id| id.parse::<u128>().ok()),
                       log_path: run.log.clone().into(),
        })
    }

    pub async fn most_recent(db: &Db, after: u64) -> Result<Vec<Run>, Box<dyn Error>> {
        let after = after as i64;
        let run = sqlx::query!(r#"SELECT r.run_id, r.log, r.start, r.end, r.client_id, j.job_id, j.name, u.name as user, j.id, j.last_progress, j.settings
                                    FROM run r
                                    JOIN job j  ON r.job_id = j.job_id
                                    JOIN user u ON j.user_id = u.user_id
                                   WHERE start > ?"#, after)
            .fetch_all(db.sql()).await?;
        Ok(run.iter().map(|row| Run { job: Job { user: row.user.clone(),
                                          id: row.id.clone(),
                                          name: row.name.clone(),
                                          job_id: row.job_id,
                                          db: db.clone(),
                                          last_progress_json: row.last_progress.clone(),
                                          settings: serde_sqlite_jsonb::from_reader(&*row.settings).unwrap_or(JobSettings::default()),
                                    },
                               date: time_from_timestamp_ms(row.start).into(),
                               duration_ms: row.end.and_then(|e| ((e as u64).checked_sub(row.start as u64))),
                               run_id: time_string_from_timestamp_ms(row.start),
                               run_db_id: row.run_id,
                               client_id: row.client_id.as_ref().and_then(|id| id.parse::<u128>().ok()),
                               log_path: row.log.clone().into(), })
           .collect())
    }

    pub async fn runs_from_ids(db: &Db, ids: &[u64]) -> Result<Vec<Run>, Box<dyn Error>> {
        let id_list = ids.iter().map(|id| id.to_string()).intersperse(",".to_string()).collect::<String>();
        #[derive(sqlx::FromRow)]
        struct Row { run_id: i64, start: i64, end: Option<i64>, client_id: Option<String>, log: String, job_id: i64, name: String, user: String, id: String, last_progress: Option<String>, settings: Vec<u8> }
        Ok(sqlx::query_as::<_, Row>(&format!(r#"SELECT r.run_id, r.start, r.end, r.client_id, r.log, j.job_id, j.name, u.name as user, j.id, j.last_progress, j.settings
                                                  FROM run r
                                                  JOIN job j ON r.job_id = j.job_id
                                                  JOIN user u ON j.user_id = u.user_id
                                                 WHERE r.run_id IN ({})"#, id_list))
           .fetch_all(db.sql()).await.map_err(|e| wrap(&e, "get runs"))?.iter()
           .map(|row|   Run { job: Job { user: row.user.clone(),
                                         id: row.id.clone(),
                                         name: row.name.clone(),
                                         job_id: row.job_id,
                                         db: db.clone(),
                                         last_progress_json: row.last_progress.clone(),
                                         settings: serde_sqlite_jsonb::from_reader(&*row.settings).unwrap_or(JobSettings::default()),
                                    },
                              date: time_from_timestamp_ms(row.start).into(),
                              duration_ms: row.end.and_then(|e| ((e as u64).checked_sub(row.start as u64))),
                              run_id: time_string_from_timestamp_ms(row.start),
                              run_db_id: row.run_id,
                              client_id: row.client_id.as_ref().and_then(|id| id.parse::<u128>().ok()),
                              log_path: row.log.clone().into(), })
           .collect())
    }

    pub fn log_path(&self)             -> PathBuf {self.job.db.db_path.join(&self.log_path)} // Full path from cwd to log
    pub fn run_path(&self)             -> PathBuf {self.job.run_path(self.date)}          // Relative path from db to run dir

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

    pub fn duration_ms(&self) -> u64 {
        self.duration_ms.unwrap_or((chrono::Local::now() - self.date).num_milliseconds() as u64)
    }

    pub fn add_stdout(&self, chunk: &str) -> Result<(), Box<dyn Error>> {
        self.mkdir_p().map_err(|e| wrap(&*e, "add_stdout"))?;

        let bytes = chunk.as_bytes();
        File::options().create(true).append(true).open(&self.log_path()).map_err(|e| wrap(&e, &format!("open {}", self.log_path().to_string_lossy())))?
            .write_all(bytes).map_err(|e| wrap(&e, &format!("write {}", self.log_path().to_string_lossy())))?;

        self.update_progress(bytes.len())?;
        Ok(())
    }

    pub fn update_progress(&self, bytes: usize) -> Result<(), Box<dyn Error>> {
        // This is the first step of progress calculation. We keep track of when the client sends up stdout in
        // a file next to the log file called "progress". Each line is a json entry--we append a timestamp and
        // the number of bytes they sent. This is used to compute the progress on the next run, not the
        // current one.

        let prog = ProgressChunk { timestamp_ms: chrono::Local::now().timestamp_millis(), bytes: bytes };
        let progress_str = serde_json::to_string(&prog)? + "\n";
        let progress_path = self.log_path().with_file_name("progress");
        File::options().create(true).append(true).open(&progress_path).map_err(|e| wrap(&e, &format!("open {}", progress_path.to_string_lossy())))?
            .write_all(progress_str.as_bytes()).map_err(|e| wrap(&e, &format!("write {}", progress_path.to_string_lossy())))?;

        Ok(())
    }

    pub async fn complete_progress(&self, end_timestamp_ms: i64) -> Result<(), Box<dyn Error>> {
        // When the job is complete we read the progress file in and squish it down to something "reasonable"
        // while adding some extra columns (we couldn't write them before since they're percentages and we
        // didn't know the total time and bytes written). Reasonable means we try to group things such that
        // each entry counts for about 10% of the time. On a job that writes a lot uniformly the whole time
        // we'll get 11 entries (an extra one and the end or beginning or something). On a job that
        // periodically spurts out lots of text it could end up with 20 entries. Either way its fairly
        // small. This gets written to the database and the progress file gets removed.

        // FIXME. All this is a first draft. It's over thought in some areas and doesn't quite work as
        // consistently as I'd hoped. It needs a 2nd (and probably 3rd) pass.

        let progress_path = self.log_path().with_file_name("progress");
        let progress_str = match std::fs::read(&progress_path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()), // No progress file means no log. And nothing to do.
            Err(e) => Err(e)?,
            Ok(bytes) => String::from_utf8(bytes)?,
        };

        if let Err(e) = std::fs::remove_file(&progress_path) { // Done with it. Don't really care if we couldn't delete it.
            warn!("Couldn't delete progress file \"{}\": {}", progress_path.to_string_lossy(), e);
        }

        let progress: Vec<ProgressChunk> = progress_str.trim().split('\n').map(|line| serde_json::from_str(line)).collect::<Result<_, _>>()?;
        if progress.len() == 0 { return Ok(()) }
        let start_timestamp_ms = self.date.timestamp_millis();
        let total_ms = (end_timestamp_ms - start_timestamp_ms) as f64;
        let total_bytes = progress.iter().rfold(0usize, |sum, &p| sum + p.bytes) as f64;
        const THRESHOLD: f64 = 0.10;
        let mut compact = Vec::new();
        let mut acc = ProgressStat::default();
        for (i, p) in progress.iter().enumerate() {
            let dt = p.timestamp_ms - if i == 0 {  start_timestamp_ms } else { progress[i-1].timestamp_ms };
            let dt_pct = dt as f64 / total_ms;
            let bytes_pct = p.bytes as f64 / total_bytes;
            if acc.dt_pct + dt_pct > THRESHOLD /* || acc.bytes_pct + bytes_pct > THRESHOLD */ {
                compact.push(acc.clone());
                acc.dt_pct = 0.0;
                acc.bytes_pct = 0.0;
            }
            acc.dt_pct += dt_pct;
            acc.bytes_pct += bytes_pct;
            acc.timestamp_ms += dt;
            acc.bytes += p.bytes;
        }
        if acc.dt_pct != 0.0 || acc.bytes_pct != 0.0 { compact.push(acc.clone()) }
        let progress_json = serde_json::to_string(&compact)?;
        sqlx::query!("UPDATE job SET last_progress = ? WHERE job_id = ?", progress_json, self.job.job_id).execute(self.job.db.sql()).await?;
        debug!("compact progress={:#?}", compact);
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
        self.complete_progress(end.unwrap()).await?;

        match self.job.prune(None).await {
            Ok(pruned) if pruned.len() > 0 => { for p in pruned.iter() { info!("{}/{}: pruned {} ({:>5}): {}", self.job.user, self.job.name, p.run_id, human_bytes(p.size), p.reason) }
                                                info!("{}/{}: total pruned: {}", self.job.user, self.job.name, human_bytes(pruned.iter().map(|p| p.size).max().unwrap())); },
            Ok(_)                          => {/* Pruned nothing, so nothing to log */},
            Err(e)                         => warn!("{}/{}: error pruning: {}", self.job.user, self.job.name, e),
        }
        Ok(())
    }

    pub fn progress(&self) -> Result<Option<serve::Progress>, Box<dyn Error>> {
        let bytes = self.log_len() as usize;
        let elapsed_ms = chrono::Local::now().timestamp_millis() - self.date.timestamp_millis();

        // This tries to estimate our progress given the data from the last progress run. It calculates time
        // just using the last time entry (since the table is divided up into time based progress perfectly
        // linear and therefore pointless to use it to try to calculate the time more granularly--this is a
        // sign the whole idea is maybe dumb).

        // Then it calulates another percentage based on the current number of output bytes. This does use the
        // table to try to relate them back to time--ideally the progress bar moves steadily as bytes come in.

        // The initial idea was that the bytes could tell us if we are moving faster or slower than normal and
        // the time could linearly adjust between spurts of output. In practice I'm not sure it works all that
        // well. And I'm not sure if the answer is to refine the algorithm or throw it out and think of
        // something else. How does Jenkins do it? Theirs seem fairly accurate.

        let Some(progress) = self.job.last_progress()? else {
            return Ok(None);
        };
        if progress.len() == 0 { return Ok(None) }
        if progress.last().unwrap().timestamp_ms == 0 { return Ok(None) }

        let last_total_ms = progress.last().unwrap().timestamp_ms as f64;
        let time_percent = match last_total_ms == 0.0 { true => None, false => Some(elapsed_ms as f64 / last_total_ms), };

        let mut percent = 0.0_f64;
        let mut prev_bytes = 0;
        for p in progress {
            percent += p.dt_pct;
            if bytes > p.bytes {
                prev_bytes = p.bytes;
                continue
            }
            let their_bytes = (p.bytes - prev_bytes) as f64;
            let my_bytes = (bytes - prev_bytes) as f64;
            let byte_percent = match their_bytes == 0.0 { true => None, false => Some(percent + p.dt_pct * my_bytes / their_bytes), };
            let ave_percent = match (byte_percent, time_percent) {
                (Some(bp), Some(tp)) => (bp + tp) / 2.0,
                (Some(bp), None)     => bp,
                (None,     Some(tp)) => tp,
                (None,     None)     => { return Ok(None) },
            };
            if ave_percent > 1.0 { return Ok(None) } // clearly we have no idea
            debug!("byte_percent = {:?}%, time_percent = {:?}%, ave = {}%", byte_percent.map(|p| p * 100.0), time_percent.map(|p| p * 100.0), ave_percent * 100.0);
            return Ok(Some(serve::Progress { percent: ave_percent as f32,
                                             eta_seconds: (last_total_ms * (1.0 - ave_percent) / 1000.0) as u32 }));
        }

        Ok(None)
    }

    pub fn log_len(&self) -> u64 {
        self.log_path().metadata().map(|m| m.len()).unwrap_or(0)
    }

    pub async fn log_file(&self) -> Result<Option<tokio::fs::File>, Box<dyn Error>> {
        if !self.log_path().is_file() { return Ok(None) }
        Ok(Some(tokio::fs::File::open(&self.log_path()).await?))
    }

    pub async fn delete(&self) -> Result<(), Box<dyn Error>> {
        let path = self.log_path();
        if path.is_file() {
            tokio::fs::remove_file(&path).await?;
            // Because we nest log dirs to keep direntry counts down ("2024/8/9/2024-08-09T00:00:01.384-07:00/log"),
            // after we've deleted the log file try to delete parent directories until we can't any more.
            let mut path = path.parent();
            loop {
                let Some(p) = path else { break };
                if tokio::fs::remove_dir(p).await.is_err() { break }
                path = p.parent();
            }
        }
        sqlx::query!("DELETE FROM run WHERE run_id = ?", self.run_db_id).execute(self.job.db.sql()).await?;
        Ok(())
    }
}

pub fn time_from_timestamp_ms(timestamp_ms: i64) -> chrono::DateTime<chrono::Local> {
    use chrono::TimeZone;
    chrono::Local.timestamp_millis(timestamp_ms).into()
}

pub fn time_string_from_timestamp_ms(timestamp_ms: i64) -> String {
    time_from_timestamp_ms(timestamp_ms).to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

#[derive(Debug, Clone)]
pub struct Settings {
    #[allow(dead_code)]
    pub db: Db,
    pub retention: RetentionSettings,
}

impl Settings {
    pub async fn load(db: &Db) -> Result<Settings, Box<dyn Error>> {
        Ok(Settings {
            db: db.clone(),
            retention: {
                if let Some(rows) = sqlx::query!("SELECT value AS retention FROM settings WHERE key = 'retention'").fetch_optional(db.sql()).await? {
                    serde_sqlite_jsonb::from_reader(&*rows.retention).unwrap_or(RetentionSettings::default())
                } else { RetentionSettings::default() }
            },
        })
    }

    pub async fn set_retention(&mut self, new_retention: RetentionSettings) -> Result<(), Box<dyn Error>> {
        let json = serde_json::to_string(&new_retention)?;
        sqlx::query!("INSERT INTO settings (key, value) VALUES ('retention', jsonb(?))
                        ON CONFLICT (key) DO UPDATE SET value=excluded.value", json) .execute(self.db.sql()).await?;
        self.retention = new_retention;
        Ok(())
    }
}

pub fn human_bytes(bytes: usize) -> String {
    if bytes == 0 { return "0B".to_string() };
    let bytes_f = bytes as f64;
    let exp = bytes_f.log(1024.0).floor();
    let exact = bytes % 1024usize.pow(exp as u32) == 0;
    let s = bytes_f / 1024f64.powi(exp as i32);
    format!("{:.*}{}", if exact {0} else {2}, s, ["B","KB","MB","GB","TB","PB","EB"][exp as usize])
}


#[cfg(test)]
mod test {
    use super::*;


    #[test]
    fn test_human_bytes() {
        assert_eq!(human_bytes(0),   "0B");
        assert_eq!(human_bytes(10),   "10B");
        assert_eq!(human_bytes(1023), "1023B");
        assert_eq!(human_bytes(1024), "1KB");
        assert_eq!(human_bytes(1025), "1.00KB");
        assert_eq!(human_bytes(1024*1024), "1MB");
        assert_eq!(human_bytes(1500*1024*1024), "1.46GB");
    }
}
