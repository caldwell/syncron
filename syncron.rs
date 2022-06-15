// Copyright © 2022 David Caldwell <david@porkrind.org>

#![feature(iter_intersperse)]

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
        let db = args.flag_db.ok_or("missing --db or SYNCRON_DB environment variable")?;
        let serve = async { serve::serve(args.flag_port, db.clone().into(), false).await.map_err(|e| format!("serve failed: {}", e)) };
        tokio::join!(serve).0?;
    }
    Ok(())
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
