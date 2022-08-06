// Copyright © 2022 David Caldwell <david@porkrind.org>

#![feature(iter_intersperse)]
#![feature(async_closure)]

#[macro_use] extern crate rocket;

use std::error::Error;

use docopt::Docopt;

mod job;
mod serve;
mod db;
mod maybe_utf8;

const USAGE: &'static str = "
Usage:
  syncron --help
  syncron -c <job-cmd>
  syncron [-h] [-v...] exec -n <name> [-i <id>] [--timeout=<timespec>] [--server=<server-url>] <job-cmd>
  syncron [-h] [-v...] serve [--db=<path>] [--port=<port>]
  syncron [-h] [-v...] db sync [--db=<path>]

Options:
  -h --help              Show this message.
  -v --verbose           Be more verbose.
  -c <job-cmd>           Shell combatible equivalent of `syncron exec <job-cmd>`
  -n --name=<name>       Job name (env: SYNCRON_NAME)
  -i --id=<job-id>       Job id (will be created from name is not specified) (env: SYNCRON_JOB_ID)
  --timeout=<timespec>   Time out job if it runs too long. Timespec is '1s, 3m, 4h', etc.
  --server=<server-url>  Base URL of a `syncron serve` instance (env: SYNCRON_SERVER)
  --db=<path-to-db>      Path to the db. Will be created if it doesn't exist [default: ./db]
                         (env: SYNCRON_DB)
  --port=<port>          Port to listen on [default: 8000] (env: SYNCRON_PORT)
";

#[derive(Debug, serde::Deserialize)]
struct Args {
    flag_verbose: usize,
    flag_db:      Option<String>,
    flag_port:    u16,
    flag_c:       Option<String>,
    flag_timeout: Option<String>,
    flag_name:    Option<String>,
    flag_job_id:  Option<String>,
    flag_server:  Option<String>,
    cmd_exec:     bool,
    cmd_serve:    bool,
    cmd_db:       bool,
    cmd_sync:     bool,
    arg_job_cmd:  String,
}

#[rocket::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let _args: Vec<String> = std::env::args().collect();
    let mut args: Args = Docopt::new(USAGE)
        .and_then(|d| d.deserialize())
        .unwrap_or_else(|e| e.exit());

    // Client settings
    env_if("SYNCRON_SERVER", |s| Ok(args.flag_server = Some(s.into())))?;
    env_if("SYNCRON_NAME",   |s| Ok(args.flag_name   = Some(s.into())))?;
    env_if("SYNCRON_JOB_ID", |s| Ok(args.flag_job_id = Some(s.into())))?;
    // Server settings
    env_if("SYNCRON_PORT",   |s| Ok(args.flag_port   = s.parse::<u16>()?))?;
    env_if("SYNCRON_DB",     |s| Ok(args.flag_db     = Some(s.into())))?;

    stderrlog::new()
        .verbosity(log::LevelFilter::Warn as usize - 1 + args.flag_verbose)
        .color(stderrlog::ColorChoice::Always)
        .show_level(true)
        .timestamp(stderrlog::Timestamp::Second)
        .module(module_path!())
        .module("reqwest")
        .module("rocket")
        .module("_") // part of rocket??
        .show_module_names(true)
        .init()?;

    debug!("args={:?}", args);

    if args.cmd_exec || args.flag_c.is_some() {
        let job_cmd = if args.cmd_exec { args.arg_job_cmd } else { args.flag_c.unwrap() };
        let server = args.flag_server.ok_or("missing --server or SYNCRON_SERVER environment variable")?.parse()?;
        let name   = args.flag_name  .ok_or("missing --name or SYNCRON_NAME environment variable")?;
        let job = job::ClientJob::new(server, &getuser(), &name, args.flag_job_id.as_deref(), args.flag_timeout.map(|s| parse_timespec(&s).unwrap()), &job_cmd).await?;
        trace!("{:?}", job);
        job.run().await?;
    }

    if args.cmd_serve {
        let db_path = args.flag_db.ok_or("missing --db or SYNCRON_DB environment variable")?;
        let sql = sqlx::SqlitePool::connect(&format!("{}/{}", &db_path, "syncron.sqlite3")).await?;
        crate::db::MIGRATOR.run(&sql).await.expect("migrate");
        let serve = async { serve::serve(args.flag_port, db_path.clone().into(), false).await.map_err(|e| format!("serve failed: {}", e)) };
        tokio::join!(serve).0?;
    } else

    if args.cmd_db && args.cmd_sync {
        let db_path: PathBuf = args.flag_db.ok_or("missing --db or SYNCRON_DB environment variable")?.into();
        let sql = sqlx::SqlitePool::connect(&format!("{}/{}", &db_path.to_string_lossy(), "syncron.sqlite3")).await?;

        let jobs_path = db_path.join("jobs");

        use std::path::{Path,PathBuf};
        pub fn dirs(dir: &Path) -> Result<Vec<String>, Box<dyn Error>> {
            if !dir.exists() { return Ok(vec![]); }
            Ok(std::fs::read_dir(dir)?
               .filter_map(|entry| entry.ok())
               .filter(|entry| match entry.metadata() { Ok(m) => m.file_type().is_dir(), _ => false })
               .map(|entry| entry.file_name().to_string_lossy().into())
               .collect())
        }

        pub struct OldServerJob {
            pub user: String,
            pub id: String,
        }

        let mut jobs: Vec<OldServerJob> = vec![];
        for user in dirs(&jobs_path)? {
            jobs.append(&mut dirs(&jobs_path.join(&user))?.iter()
                        .map(|id| OldServerJob{user: user.clone(), id: id.to_string() })
                        .collect())
        }

        for job in jobs.iter() {
            #[derive(Debug, serde::Serialize, serde::Deserialize)]
            pub struct JobInfo {
                pub name: String,
            }
            let info_path = jobs_path.join(&job.user).join(&job.id).join("info");
            if !info_path.is_file() { warn!("No info file!"); continue; }
            let info: JobInfo = serde_json::from_slice(&std::fs::read(info_path)?)?;

            sqlx::query!("INSERT INTO user (name) VALUES (?) ON CONFLICT DO NOTHING", job.user)
                .execute(&sql).await?;
            let user_id = sqlx::query!("SELECT user_id FROM user WHERE name = ?", job.user)
                .fetch_one(&sql).await?
                .user_id;
            let job_name = info.name;
            sqlx::query!("INSERT INTO job (user_id, id, name) values (?, ?, ?) ON CONFLICT DO NOTHING", user_id, job.id, job_name)
                .execute(&sql).await?;

            let job_id = sqlx::query!("SELECT job_id FROM job WHERE user_id = ? and id = ?", user_id, job.id)
                    .fetch_one(&sql).await?
                    .job_id;

            let job_path = jobs_path.join(&job.user).join(&job.id);
            let paths = dirs(&job_path)?;
            let mut runs = paths.iter()
               .filter_map(|name| chrono::DateTime::parse_from_rfc3339(name).map(|d| (d,name)).ok())
               .collect::<Vec<(chrono::DateTime<chrono::FixedOffset>, &String)>>();
            runs.sort_by(|a,b| a.0.cmp(&b.0));
            #[derive(Debug, PartialEq)]
            pub struct OldServerRun {
                pub date: chrono::DateTime<chrono::FixedOffset>,
                pub run_id: String,
                pub client_id: Option<u128>,
            }
            let runs: Vec<OldServerRun> = runs.into_iter().map(|run_date| OldServerRun{ date: run_date.0, run_id: run_date.1.clone(), client_id: None }).collect();

            for run in runs.into_iter() {
                let run_path = job_path.join(&run.run_id);
                let info_path = run_path.join("info");
                if !info_path.is_file() { warn!("No info file: {}", info_path.to_string_lossy()); continue }
                use crate::maybe_utf8::MaybeUTF8;
                #[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
                pub struct OldServerRunInfo {
                    pub cmd:    String,
                    pub env:    Vec<(MaybeUTF8,MaybeUTF8)>,
                    pub end:    Option<chrono::DateTime<chrono::Utc>>,
                    pub status: Option<serve::ExitStatus>,
                }
                let info = {
                    let info: Result<OldServerRunInfo,_> = serde_json::from_slice(&std::fs::read(&info_path).map_err(|e| wrap(&e, &format!("read {}", info_path.to_string_lossy())))?)
                        .map_err(|e| wrap(&e, &format!("parse {}", info_path.to_string_lossy())));
                    if let Err(e) = info { warn!("{}", e); continue; }
                    info.unwrap()
                };

                print!("\r\x1b[2KImporting run for {} [{}]...", job.id, run.run_id);
                use std::io::Write;
                let _ = std::io::stdout().flush();

                let env = serde_json::to_string(&info.env)?;
                let log_path = run_path.join("log");
                let log_path = log_path.as_os_str().to_str().ok_or(format!("bad path: {:?}", log_path))?.to_owned();
                let start = run.date.timestamp_millis();
                let end = info.end.map(|t| t.timestamp_millis());
                let status = serde_json::to_string(&info.status)?;
                sqlx::query!(r"INSERT INTO run (job_id, cmd, env, log, start, end, status)
                                      VALUES (?, ?, ?, ?, ?, ?, ?)
                                      ON CONFLICT DO UPDATE SET cmd    = excluded.cmd,
                                                                env    = excluded.env,
                                                                log    = excluded.log,
                                                                end    = excluded.end,
                                                                status = excluded.status",
                             job_id, info.cmd, env, log_path, start, end, status)
                    .execute(&sql).await?;
            }
        }
    }

    Ok(())
}

fn wrap<E: Error>(e: E, s: &str) -> Box<dyn Error> {
    Box::<dyn Error>::from(format!("{}: {:?}", s, e))
}

fn wrap_str<E: Error>(e: E, s: &str) -> String {
    format!("{}: {:?}", s, e)
}

fn parse_timespec(s: &str) -> Result<std::time::Duration, Box<dyn Error>> {
    let suffix_at = s.find(|c:char | !c.is_numeric());
    if suffix_at.is_none() {
        return Ok(std::time::Duration::new(s.parse()?, 0));
    }
    let (value, unit) = s.split_at(suffix_at.unwrap());
    let val:u64 = value.parse()?;
    Ok(match unit {
        "s" => std::time::Duration::new(val * 1, 0),
        "m" => std::time::Duration::new(val * 60, 0),
        "h" => std::time::Duration::new(val * 60 * 60, 0),
        "d" => std::time::Duration::new(val * 60 * 60 * 24, 0),
        _ => Err(format!("Bad time unit: {}", s))?,
    })
}

fn env_if<T, F: FnOnce(&str) -> Result<T,Box<dyn Error>>>(name: &str, f: F) -> Result<Option<T>,String> {
    if let Some(value) = std::env::var_os(name) {
        let s = value.into_string().map_err(|_| format!("Bad unicode in {}", name))?;
        let v = f(&s).map_err(|e| format!("{}: {}", name, e));
        Ok(Some(v?))
    } else {
        Ok(None)
    }
}

fn getuser() -> String {
    let uid = nix::unistd::getuid();
    let fallback = format!("uid={}", uid);
    nix::unistd::User::from_uid(uid).map_or(fallback.clone(), |po| po.map_or(fallback, |p| p.name))
}
