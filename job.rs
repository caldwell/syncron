// Copyright © 2022 David Caldwell <david@porkrind.org>

use std::error::Error;
use std::ffi::OsString;
use std::fs::File;
use std::io::Write;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path,PathBuf};

use reqwest::{header,Url};

use crate::serve;
use crate::db::Db;
use crate::maybe_utf8::MaybeUTF8;

#[derive(Debug)]
pub struct ClientJob {
    id:       String,
    run_id:   String, // just for tests, not a really good api
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
        Ok(ClientJob { id:resp.id, run_id: resp.run_id, timeout:timeout, cmd:cmd.to_string(), api: api })
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

    async fn copy_output<T: tokio::io::AsyncRead+Unpin >(api: Api, mut from: T, id: String, kind: serve::OutKind) {
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

#[derive(Debug, PartialEq, Clone)]
pub struct ServerJob {
    pub user: String,
    pub id: String,
    pub db: Db,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct JobInfo {
    pub name: String,
}

#[derive(Debug, PartialEq)]
pub struct ServerRun {
    pub job: ServerJob,
    pub date: chrono::DateTime<chrono::FixedOffset>,
    pub run_id: String,
    pub client_id: Option<u128>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct ServerRunInfo {
    pub cmd:    String,
    pub env:    Vec<(MaybeUTF8,MaybeUTF8)>,
    pub end:    Option<chrono::DateTime<chrono::Utc>>,
    pub status: Option<serve::ExitStatus>,
}

pub fn slug(st: &str) -> String {
    let mut slug = st.replace(|ch: char| !ch.is_ascii_alphanumeric(), "-");
    slug.make_ascii_lowercase();
    slug.split('-').filter(|s| !s.is_empty()).intersperse("-").collect::<String>()
}

impl ServerJob {
    pub fn create(db: &Db, user: &str, name: &str, id: Option<&str>) -> Result<ServerJob, Box<dyn Error>> {
        let job = ServerJob::new(db, user, id.unwrap_or(&slug(name)))?;
        job.set_name(name)?;
        Ok(job)
    }

    pub fn new(db: &Db, user: &str, id: &str) -> Result<ServerJob, Box<dyn Error>> {
        if user.is_empty() || user.contains("/") || user.starts_with(".") { Err(format!("Bad user"))? }
        if id.is_empty()   || id.contains("/")   || id.starts_with(".")   { Err(format!("Bad id"))? }
        Ok(ServerJob { db:   db.clone(),
                       user: user.to_string(),
                       id:   id.to_string(),
        })
    }

    pub fn job_path(&self)  -> PathBuf {self.db.jobs_path().join(&self.user).join(&self.id)}
    pub fn info_path(&self) -> PathBuf {self.db.jobs_path().join(&self.user).join(&self.id).join("info")}

    fn mkdir_p(&self) -> Result<(), std::io::Error> {
        std::fs::DirBuilder::new().recursive(true).create(self.job_path())
    }

    pub fn runs(&self) -> Result<Vec<ServerRun>, Box<dyn Error>> {
        let paths = self.db.dirs(&self.job_path())?;
        let mut runs = paths.iter()
           .filter_map(|name| chrono::DateTime::parse_from_rfc3339(name).map(|d| (d,name)).ok())
           .collect::<Vec<(chrono::DateTime<chrono::FixedOffset>, &String)>>();
        runs.sort_by(|a,b| a.0.cmp(&b.0));
        Ok(runs.into_iter().map(|run_date| ServerRun{ job: self.clone(), date: run_date.0, run_id: run_date.1.clone(), client_id: None }).collect())
    }

    pub fn latest_run(&self) -> Result<Option<ServerRun>, Box<dyn Error>> {
        return Ok(self.runs()?.pop());
    }

    pub fn run(&self, run_id: &str) -> Result<ServerRun, Box<dyn Error>> {
        if run_id.is_empty() || run_id.contains("/") || run_id.starts_with(".") { Err(format!("Bad user"))? }
        let run = ServerRun{ job: self.clone(), date: chrono::DateTime::parse_from_rfc3339(run_id)?, run_id: run_id.to_string(), client_id: None };
        if !run.exists() { Err("Run not found")? }
        Ok(run)
    }

    pub fn set_name(&self, name: &str) -> Result<(), Box<dyn Error>> {
        self.mkdir_p()?;
        File::create(self.info_path())?.write_all(&serde_json::to_vec(&JobInfo{ name: name.to_string() })?)?;
        Ok(())
    }

    pub fn info(&self) -> Result<JobInfo, Box<dyn Error>> {
        let path = self.info_path();
        if !path.is_file() { Err("No info file!")? }
        Ok(serde_json::from_slice(&std::fs::read(path)?)?)
    }

    pub fn name(&self) -> Result<String, Box<dyn Error>> {
        Ok(self.info()?.name)
    }
}

impl ServerRun {
    pub fn create(db: PathBuf, user: &str, name:&str, id:Option<&str>) -> Result<ServerRun, Box<dyn Error>> {
        let mut client_id_bytes = [0; 128/8];
        getrandom::getrandom(&mut client_id_bytes)?;
        let client_id: u128 = u128::from_ne_bytes(client_id_bytes);
        let date = chrono::Local::now();
        let run = ServerRun { job: ServerJob::create(&Db::new(&db), user, name, id)?,
                              date: date.into(),
                              run_id: date.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                              client_id: Some(client_id)};
        std::fs::DirBuilder::new().recursive(true).create(run.job.db.ids_path())?;
        std::os::unix::fs::symlink(run.run_path(), run.job.db.id_path(client_id))?;
        Ok(run)
    }

    pub fn from_client_id(db_path: PathBuf, id: u128) -> Result<ServerRun, Box<dyn Error>> {
        let db = Db::new(&db_path);
        let p = db.id_path(id);
        let path = std::fs::read_link(&p).map_err(|e| format!("{:?}: {}", p, e))?;
        let rel = path.strip_prefix(db.jobs_path())?;
        let mut parts = rel.iter();
        let user = Path::new(parts.next().ok_or(format!("no user in symlink {:?}", rel))?);
        let job_id = Path::new(parts.next().ok_or(format!("no id in symlink {:?}", rel))?);
        let run_id = parts.next().ok_or(format!("no run_id iny symlink {:?}", rel))?;
        Ok(ServerRun { job: ServerJob::new(&db, &user.to_string_lossy(), &job_id.to_string_lossy())?,
                       run_id: run_id.to_string_lossy().into(),
                       date: chrono::DateTime::parse_from_rfc3339(&run_id.to_string_lossy())?,
                       client_id: Some(id),
        })
    }

    pub fn run_path(&self)             -> PathBuf {self.job.job_path().join(&self.run_id)}
    pub fn info_path(&self)            -> PathBuf {self.run_path().join("info")}
    pub fn log_path(&self)             -> PathBuf {self.run_path().join("log")}
    pub fn progress_path(&self)        -> PathBuf {self.run_path().join("progress")}
    pub fn heartbeat_path(&self)       -> PathBuf {self.run_path().join("heartbeat")}

    fn mkdir_p(&self) -> Result<(), std::io::Error> {
        std::fs::DirBuilder::new().recursive(true).create(self.run_path())
    }

    pub fn exists(&self) -> bool {
        self.info_path().is_file()
    }

    pub fn info(&self) -> Result<ServerRunInfo, Box<dyn Error>> {
        let path = self.info_path();
        if !path.is_file() { Err("No info file!")? }
        let mut info: ServerRunInfo = serde_json::from_slice(&std::fs::read(path)?)?;
        if info.status.is_none() {
            if let Ok(ts) = self.heartbeat() {
                if chrono::Local::now().timestamp() - ts > 30 {
                    info = self.complete_with_info(&info, serve::ExitStatus::ServerTimeout)?;
                }
            }
        }
        Ok(info)
    }

    pub fn set_info(&self, info: &ServerRunInfo) -> Result<(), Box<dyn Error>> {
        self.mkdir_p()?;
        File::create(self.info_path())?.write_all(&serde_json::to_vec(&info)?)?;
        Ok(())
    }

    pub fn add_stdout(&self, chunk: &str) -> Result<(), Box<dyn Error>> {
        self.mkdir_p()?;
        File::options().create(true).append(true).open(self.log_path())?.write_all(chunk.as_bytes())?;
        Ok(())
    }

    pub fn set_heartbeat(&self) -> Result<(), Box<dyn Error>> {
        let ts = format!("{}", chrono::Local::now().timestamp());
        atomic_file_write(&self.heartbeat_path(), ts.as_bytes())?;
        Ok(())
    }

    pub fn heartbeat(&self) -> Result<i64, Box<dyn Error>> {
        Ok(String::from_utf8_lossy(&std::fs::read(&self.heartbeat_path())?).parse::<i64>()?)
    }

    pub fn complete(&self, status: serve::ExitStatus) -> Result<ServerRunInfo, Box<dyn Error>> {
        self.complete_with_info(&self.info()?, status)
    }

    pub fn complete_with_info(&self, info: &ServerRunInfo, status: serve::ExitStatus) -> Result<ServerRunInfo, Box<dyn Error>> {
        let mut new_info: ServerRunInfo = info.clone();
        new_info.end = Some(chrono::Local::now().into());
        new_info.status = Some(status);
        self.set_info(&new_info)?;
        if let Some(client_id) = self.client_id {
            std::fs::remove_file(self.job.db.ids_path().join(&format!("{}", client_id)))?;
        }
        Ok(new_info)
    }

    pub fn progress(&self) -> Result<Option<serve::Progress>, Box<dyn Error>> {
        Ok(None)
    }

    pub fn log_len(&self) -> u64 {
        self.log_path().metadata().map(|m| m.len()).unwrap_or(0)
    }

    pub fn log(&self, seek: Option<u64>) -> Result<Option<String>, Box<dyn Error>> {
        use std::io::{Seek,Read};
        let path = self.log_path();
        if !path.is_file() { return Ok(None) }
        let mut f = std::fs::File::open(path)?;
        if let Some(bytes) = seek {
            f.seek(std::io::SeekFrom::Start(bytes))?;
        }
        let mut buf = vec![];
        f.read_to_end(&mut buf)?;
        Ok(Some(String::from_utf8_lossy(&buf).to_string()))
        //Ok(Some(String::from_utf8_lossy(&std::fs::read(self.log_path())?).to_string()))
    }
}

pub fn atomic_file_write(path: &Path, bytes: &[u8]) -> Result<(), Box<dyn Error>> {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let tempname = path.with_extension(format!("new-{:x}", rng.gen::<u64>()));
    File::create(&tempname)?.write_all(bytes)?;
    std::fs::rename(tempname, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir() -> tempfile::TempDir {
        tempfile::Builder::new().prefix("syncron-test").tempdir().unwrap()
    }

    macro_rules! assert_file_eq {
        ( $path:expr, $contents:expr ) => {
            let b = std::fs::read($path).expect(&$path.to_string_lossy());
            assert_eq!(String::from_utf8_lossy(&b), $contents);
        }
    }

    #[test]
    fn basic() {
        let db = test_dir();
        let cmd = "echo a simple test";
        let run = ServerRun::create(db.path().into(), "test-user", "David's The _absolute_ Greatest", None).expect("ServerRun create worked");
        assert_eq!(run.job.id, "david-s-the-absolute-greatest");
        let env = vec![
            (MaybeUTF8::new(OsString::from("PATH")),            MaybeUTF8::new(OsString::from("Something:something_else/"))),
            (MaybeUTF8::new(OsString::from("HOME")),            MaybeUTF8::new(OsString::from("/home/my-home-dir"))),
            (MaybeUTF8::new(OsString::from("SOMETHING WACKY")), MaybeUTF8::new(OsString::from("Oh\nDear"))),
        ];
        run.set_info(&ServerRunInfo{
            cmd:    cmd.to_string(),
            env:    env,
            end:    None,
            status: None,
        }).expect("set_info worked");

        let id = run.client_id.expect("got client_id");
        let mut run2 = ServerRun::from_client_id(db.path().into(), id).expect("ServerRun::from_client_id()");
        assert!(run.date - run2.date < chrono::Duration::milliseconds(1000), "Dates are less close than expected {} vs {}", run.date, run2.date);
        run2.date = run.date;
        assert_eq!(run, run2);
        //println!("run2={:?}", run2);
        run2.add_stdout("Some text. ").expect("text added");
        run2.add_stdout("Some more text.\n").expect("more text added");
        run2.add_stdout("Even more text.\n").expect("even more text added");
        // run.add_stderr("What, an error?\n");
        run2.complete(serve::ExitStatus::Exited(0)).expect("completed with no errors");

        //let _=std::process::Command::new("find").arg(db.path()).arg("-ls").status();
        let info = serde_json::from_slice::<ServerRunInfo>(&std::fs::read(db.path().join("jobs").join("test-user").join("david-s-the-absolute-greatest").join(&run.run_id).join("info"))
                                                                         .expect("info file exists"))
                                         .expect("Info file is valid json");
        assert_eq!(info.cmd, cmd);
        assert_file_eq!(&db.path().join("jobs").join("test-user").join("david-s-the-absolute-greatest").join(&run.run_id).join("log"), "Some text. Some more text.\nEven more text.\n");

        let run3 = ServerRun::from_client_id(db.path().into(), id);
        assert!(run3.is_err(), "ServerRun::from_client_id() returned was {:?}", run3);
    }

    #[test]
    fn heartbeat_timeout() {
        let db = test_dir();
        let cmd = "echo a simple test";
        let run = ServerRun::create(db.path().into(), "test-user", "David's The _absolute_ Greatest", None).expect("ServerRun create worked");
        run.set_info(&ServerRunInfo{
            cmd:    cmd.to_string(),
            env:    vec![],
            end:    None,
            status: None,
        }).expect("set_info worked");

        atomic_file_write(&run.heartbeat_path(), "0".as_bytes()).expect("manually write old heartbeat");
        let info = run.info().expect("got info");
        assert_eq!(info.status, Some(serve::ExitStatus::ServerTimeout));
    }


    //#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[tokio::test]
    async fn integration() {
        let db = test_dir();
        let cmd = "echo a simple test";
        std::env::set_var("MY_ENV_VAR", "some value");
        let db_path = db.path().to_path_buf();
        let _serve = tokio::spawn(async move { serve::serve(32923, db_path, true).await.unwrap(); });
        let _client = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await; // HACK
            let job = crate::job::ClientJob::new("http://127.0.0.1:32923/".parse().unwrap(), "test-user", "My Job", Some("my-id"), None, cmd).await.unwrap();
            job.run().await.expect("job ran");
            //let _=std::process::Command::new("find").arg(db.path()).arg("-ls").status();
            let info = serde_json::from_slice::<ServerRunInfo>(&std::fs::read(db.path().join("jobs").join("test-user").join("my-id").join(&job.run_id).join("info"))
                                                                             .expect("info file exists"))
                                             .expect("Info file is valid json");
            assert_eq!(info.cmd, "echo a simple test");
            assert_file_eq!(&db.path().join("jobs").join("test-user").join("my-id").join(&job.run_id).join("log"), "a simple test\n");
            //std::process::Command::new("find").arg(db.path()).arg("-ls").status();

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
        let db = test_dir();
        let cmd = "sleep 10";
        let db_path = db.path().to_path_buf();
        let _serve = tokio::spawn(async move { serve::serve(32924, db_path, true).await.unwrap(); });
        let _client = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await; // HACK
            let job = crate::job::ClientJob::new("http://127.0.0.1:32924/".parse().unwrap(), "test-user", "My Bad Job", None, Some(std::time::Duration::from_millis(1500)), cmd).await.unwrap();
            job.run().await.expect("job ran");
            assert_eq!(db.path().join("jobs").join("test-user").join("my-bad-job").join(&job.run_id).join("heartbeat").exists(), true);
            //let _=std::process::Command::new("find").arg(db.path()).arg("-ls").status();
            let info = serde_json::from_slice::<ServerRunInfo>(&std::fs::read(db.path().join("jobs").join("test-user").join("my-bad-job").join(&job.run_id).join("info"))
                                                                             .expect("info file exists"))
                                             .expect("Info file is valid json");
            assert_eq!(info.cmd, "sleep 10");
            assert_eq!(info.status, Some(serve::ExitStatus::ClientTimeout));
            assert_eq!(db.path().join("jobs").join("test-user").join("my-bad-job").join(&job.run_id).join("log").exists(), false);

            job.api.post("/shutdown", &[]).await.expect("POST /shutdown");
        }).await.unwrap();
        _serve.await.unwrap();
    }
}
