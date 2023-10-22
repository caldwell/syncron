// Copyright © 2022 David Caldwell <david@porkrind.org>

#![feature(iter_intersperse)]
#![feature(async_closure)]

#[macro_use] extern crate rocket;

use std::error::Error;

use docopt::Docopt;

mod client;
mod serve;
mod db;
mod maybe_utf8;

const USAGE: &'static str = r#"
Usage:
  syncron --help
  syncron -c <job-cmd>
  syncron [-h] [-v...] exec (-n <name> | -i <id> | -n <name> -i <id>) [--timeout=<timespec>] [--server=<server-url>] <job-cmd>
  syncron [-h] [-v...] serve [--db=<path>] [--port=<port>]

Options:
  -h --help              Show this message.
  -v --verbose           Be more verbose.
  -c <job-cmd>           Shell combatible equivalent of `syncron exec <job-cmd>`
  -n --name=<name>       Job name (env: SYNCRON_NAME)
  -i --id=<job-id>       Job id (will be created from name is not specified)
                         To specify the job id from the environment, set SYNCRON_NAME to
                         "@<job-id>" or "@<job-id> <job-name>".
  --timeout=<timespec>   Time out job if it runs too long. Timespec is '1s, 3m, 4h', etc.
  --server=<server-url>  Base URL of a `syncron serve` instance (env: SYNCRON_SERVER)
  --db=<path-to-db>      Path to the db. Will be created if it doesn't exist [default: ./db]
                         (env: SYNCRON_DB)
  --port=<port>          Port to listen on [default: 8000] (env: SYNCRON_PORT)
"#;

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
    env_if("SYNCRON_NAME",   |s| Ok({ let (job_id, name) = parse_name_env(s);
                                      if job_id.is_some() { args.flag_job_id = job_id }
                                      if name.is_some()   { args.flag_name   = name } }))?;
    // Server settings
    env_if("SYNCRON_PORT",   |s| Ok(args.flag_port   = s.parse::<u16>()?))?;
    env_if("SYNCRON_DB",     |s| Ok(args.flag_db     = Some(s.into())))?;

    use tracing_subscriber::fmt::format::FmtSpan;
    let (env_filter, span_events) = match args.flag_verbose {
        0   => ("warn",                                        FmtSpan::NONE,                  ),
        1   => ("warn,syncron=info",                           FmtSpan::NONE,                  ),
        2   => ("warn,syncron=debug",                          FmtSpan::NONE,                  ),
        3   => ("warn,syncron=trace",                          FmtSpan::NONE,                  ),
        4   => ("warn,syncron=trace",                          FmtSpan::NEW | FmtSpan::CLOSE,  ),
        5   => ("warn,syncron=trace,rocket=trace",             FmtSpan::NEW | FmtSpan::CLOSE,  ),
        6   => ("warn,syncron=trace,rocket=trace,hyper=debug", FmtSpan::NEW | FmtSpan::CLOSE,  ),
        7.. => ("trace",                                       FmtSpan::NEW | FmtSpan::CLOSE,  ),
        _ => todo!(),
    };

    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_span_events(span_events)
        .with_timer(tracing_subscriber::fmt::time::LocalTime::new(time::macros::format_description!("")))
        .with_ansi(true)
        .compact()
        .with_writer(std::io::stderr)
        .with_level(true)
        .init();

    tracing::debug!("args={:?}", args);

    if args.cmd_exec || args.flag_c.is_some() {
        let job_cmd = if args.cmd_exec { args.arg_job_cmd } else { args.flag_c.unwrap() };
        let server: reqwest::Url = args.flag_server.ok_or("missing --server or SYNCRON_SERVER environment variable")?.parse()?;
        let name   = args.flag_name  .ok_or("missing --name or SYNCRON_NAME environment variable")?;
        let timeout = args.flag_timeout.map(|s| parse_timespec(&s).unwrap());
        let result = client::Job::new(server.clone(), &getuser(), &name, args.flag_job_id.as_deref(), timeout, &job_cmd).await.map_err(|e| e.to_string());
        match result {
            Ok(job) => {
                trace!("{:?}", job);
                job.run().await?;
            },
            Err(e) => {
                warn!("Failed to connect to server {}: {}. Running job in fallback mode.", server, e);
                client::fallback_run(timeout, &job_cmd).await?;
            }
        }
    }

    if args.cmd_serve {
        let db_path = args.flag_db.ok_or("missing --db or SYNCRON_DB environment variable")?;
        let db = db::Db::new(&std::path::PathBuf::from(db_path.clone())).await?;
        let serve = async { serve::serve(args.flag_port, &db, false).await.map_err(|e| format!("serve failed: {}", e)) };
        tokio::join!(serve).0?;
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

/// Returns (job_id, name)
fn parse_name_env(s: &str) -> (Option<String>, Option<String>) {
    match s.strip_prefix('@').map(|jn| jn.split_once(' ').ok_or(jn)) {
        Some(Ok((job_id, name))) => (Some(job_id.to_owned()), Some(name.to_owned())),
        Some(Err(job_id))        => (Some(job_id.to_owned()), None),
        None                     => (None,                    Some(s.to_owned())),
    }
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
