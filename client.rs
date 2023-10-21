// Copyright © 2022 David Caldwell <david@porkrind.org>

use std::error::Error;
use std::ffi::OsString;
use std::os::unix::process::ExitStatusExt;

use reqwest::{header,Url};
use reqwest::header::{CONTENT_TYPE, ACCEPT};

use crate::serve;
use crate::maybe_utf8::MaybeUTF8;

#[derive(Debug)]
pub struct Job {
    id:       String,
    timeout:  Option<std::time::Duration>,
    cmd:      String,
    api:      Api,
}

impl Job {
    pub async fn new(server_url: Url, user: &str, name: &str, id: Option<&str>, timeout: Option<std::time::Duration>, cmd: &str) -> Result<Job, Box<dyn Error>> {
        let mut env=vec![];
        for (k, v) in std::env::vars_os() {
            env.push((MaybeUTF8::new(k),MaybeUTF8::new(v)));
        }
        let api = Api::new(server_url)?;
        let resp: serve::CreateRunResp = serde_json::from_str(&api.post("/run/create", &serde_json::to_string(&serve::CreateRunReq{ user:user.to_string(), name:name.to_string(), id:id.map(|i|i.to_string()), cmd:cmd.to_string(), env:env })?.as_bytes()).await?)?;
        Ok(Job { id:resp.id, timeout:timeout, cmd:cmd.to_string(), api: api })
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

        let outpiper = Job::copy_output(self.api.clone(), child.stdout.take().unwrap(), self.id.clone(), serve::OutKind::Stdout);
        let errpiper = Job::copy_output(self.api.clone(), child.stderr.take().unwrap(), self.id.clone(), serve::OutKind::Stderr);
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
        fake_browser_headers.insert(ACCEPT, header::HeaderValue::from_static("application/json"));
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
            .header(CONTENT_TYPE, "application/json")
            .body(bod)
            .send()
            .await?;

        resp.error_for_status_ref()?;
        use std::os::unix::ffi::OsStringExt;
        trace!("API: {} <- {:?}", self.server.join(path)?, OsString::from_vec(body.to_vec()).to_string_lossy());
        Ok(resp.text().await?)
    }

    pub async fn get(&self, path: &str) -> Result<String, Box<dyn Error>> {
        let resp = self.ua.get(self.server.join(path)?)
            .send()
            .await?;

        resp.error_for_status_ref()?;
        let resp_str = resp.text().await?;
        trace!("API: {} -> {}", self.server.join(path)?, resp_str);
        Ok(resp_str)
    }
}

pub async fn fallback_run(timeout: Option<std::time::Duration>, cmd: &str)  -> Result<(), Box<dyn Error>> {
    // This is largely a copy+paste of Job::run(), above, but I don't know that it's worth it to abstract and de-duplicate.
    use tokio::process::Command;

    let shell = match (std::env::var("SYNCRON_SHELL"), std::env::var("SHELL"), std::env::args().nth(0)) {
        (Ok(sh), _,      _)                    => sh,
        (_,      Ok(sh), Some(me)) if sh != me => sh, // Prevent recursion
        (_,      Ok(sh), None)                 => sh, // Probably will never happen
        (_,      _,      _)                    => "/bin/sh".to_string(),
    };
    let mut child = Command::new(shell).args([OsString::from("-c"), OsString::from(&cmd)])
        .stdin(std::process::Stdio::null())
        .spawn()?;

    trace!("Spawned child in fallback mode{:?}", child);

    let heartbeat = async {
        let now = std::time::Instant::now();
        loop {
            trace!("Waiting 1 second");
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            if timeout.is_some() && now.elapsed() > timeout.unwrap() {
                trace!("Timed out");
                return Err(now.elapsed());
            }
        }
        #[allow(unreachable_code)] Ok(()) // if I remove this, it errs.
    };
    loop {
        tokio::select! {
            status     = child.wait()    => {
                match status {
                    Err(e) => {
                        error!("Child process failed: {}", e);
                        Err(e)?;
                        break;
                    },
                    Ok(status) => {
                        trace!("Child exited with {}", status);
                        if let Some(code) = status.code() {
                            std::process::exit(code); // pass it on
                        }
                        break;
                    }
                }
            },
            Err(elapsed) = heartbeat => {
                error!("Timeout reached after {:?}! Killing child {:?}", elapsed, child);
                child.kill().await?;
                break;
            },
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use chrono::Datelike;

    async fn test_db() -> (db::Db, tempfile::TempDir) {
        let db_path = tempfile::Builder::new().prefix("syncron-test").tempdir().unwrap();
        let db = db::Db::new(&db_path.path()).await.expect(&format!("Create db in {}", db_path.path().to_string_lossy()));
        (db, db_path)
    }

    impl PartialEq for db::Job {
        fn eq(&self, a: &db::Job) -> bool {
            self.user    == a.user   &&
            self.id      == a.id     &&
            self.name    == a.name   &&
            self.job_id  == a.job_id
        }
    }

    impl PartialEq for db::Run {
        fn eq(&self, a: &db::Run) -> bool {
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
        let (db, db_path) = test_db().await;
        let cmd = "echo a simple test";
        let env = vec![
            (MaybeUTF8::new(OsString::from("PATH")),            MaybeUTF8::new(OsString::from("Something:something_else/"))),
            (MaybeUTF8::new(OsString::from("HOME")),            MaybeUTF8::new(OsString::from("/home/my-home-dir"))),
            (MaybeUTF8::new(OsString::from("SOMETHING WACKY")), MaybeUTF8::new(OsString::from("Oh\nDear"))),
        ];
        let run = db::Run::create(&db, "test-user", "David's The _absolute_ Greatest", None, cmd.to_string(), env).await.expect("db::Run create worked");
        assert_eq!(run.job.id, "david-s-the-absolute-greatest");

        let id = run.client_id.expect("got client_id");
        let mut run2 = db::Run::from_client_id(&db, id).await.expect("db::Run::from_client_id()");
        assert!(run.date - run2.date < chrono::Duration::milliseconds(1000), "Dates are less close than expected {} vs {}", run.date, run2.date);
        run2.date = run.date;
        assert_eq!(run, run2);
        run2.add_stdout("Some text. ").expect("text added");
        run2.add_stdout("Some more text.\n").expect("more text added");
        run2.add_stdout("Even more text.\n").expect("even more text added");
        run2.complete(serve::ExitStatus::Exited(0)).await.expect("completed with no errors");

        assert_file_eq!(&db_path.path().join("jobs").join("test-user").join("david-s-the-absolute-greatest")
                        .join(run2.date.year().to_string()).join(run2.date.month().to_string()).join(run2.date.day().to_string())
                        .join(&run.run_id).join("log"), "Some text. Some more text.\nEven more text.\n");

        let run3 = db::Run::from_client_id(&db, id).await;
        assert!(run3.is_err(), "db::Run::from_client_id() returned was {:?}", run3);
    }

    #[tokio::test]
    async fn heartbeat_timeout() {
        let (db, db_path) = test_db().await;
        let cmd = "echo a simple test";
        let run = db::Run::create(&db, "test-user", "David's The _absolute_ Greatest", None, cmd.to_string(), vec![]).await.expect("db::Run create worked");

        sqlx::query!("UPDATE run SET heartbeat = 0 WHERE run_id = ?", run.run_db_id).execute(db.sql()).await.expect("update run set heartbeat");
        let info = run.info().await.expect("got info");
        assert_eq!(info.status, Some(serve::ExitStatus::ServerTimeout));
    }

    #[tokio::test]
    async fn integration() {
        let (db, db_path) = test_db().await;
        let cmd = "echo a simple test";
        std::env::set_var("MY_ENV_VAR", "some value");
        let db_path = db_path.path().to_path_buf();
        let _serve = tokio::spawn({ let db = db.clone(); async move { serve::serve(32923, &db, true).await.unwrap(); }});
        let _client = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await; // HACK
            let job = crate::client::Job::new("http://127.0.0.1:32923/".parse().unwrap(), "test-user", "My Job", Some("my-id"), None, cmd).await.unwrap();
            let log_path = sqlx::query!("SELECT log FROM run WHERE client_id = ?", job.id).fetch_one(db.sql()).await.expect("SELECT log FROM run").log;
            job.run().await.expect("job ran");
            assert_file_eq!(&db_path.join(&log_path), "a simple test\n");

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
        let (db, db_path) = test_db().await;
        let cmd = "sleep 10";
        let db_path = db_path.path().to_path_buf();
        let _serve = tokio::spawn({let db = db.clone(); async move { serve::serve(32924, &db, true).await.unwrap(); }});
        let _client = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await; // HACK
            let job = crate::client::Job::new("http://127.0.0.1:32924/".parse().unwrap(), "test-user", "My Bad Job", None, Some(std::time::Duration::from_millis(1500)), cmd).await.unwrap();
            let log_path = sqlx::query!("SELECT log FROM run WHERE client_id = ?", job.id).fetch_one(db.sql()).await.expect("SELECT log FROM run").log;
            job.run().await.expect("job ran");
            assert_eq!(db_path.join(&log_path).exists(), false);

            job.api.post("/shutdown", &[]).await.expect("POST /shutdown");
        }).await.unwrap();
        _serve.await.unwrap();
    }
}
