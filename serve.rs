// Copyright © 2022 David Caldwell <david@porkrind.org>

use std::error::Error;
use std::io::Read;
use std::path::{Path,PathBuf};

use rocket::http::ContentType;
use rocket::request::Request;
use rocket::response::{Debug,Redirect, Responder, Response};
use rocket::serde::{Serialize, Deserialize, json::Json};
use rocket::State;

use crate::db;
use crate::db::Db;
use crate::maybe_utf8::MaybeUTF8;
use crate::{wrap,wrap_str};

type WebResult<T, E = Debug<Box<dyn Error>>> = std::result::Result<T, E>; // What is this magic??

#[get("/")]
#[tracing::instrument(name="GET /")]
async fn index() -> Option<(ContentType, String)> {
    files("index.html".into()).await
}

#[get("/<file..>")]
#[tracing::instrument(name="GET /<file..>")]
async fn files(file: PathBuf) -> Option<(ContentType, String)> {
    file_from_zip_or_fs(&Path::new("web/").join(file))
}

fn file_from_zip_or_fs(file: &Path) -> Option<(ContentType, String)> {
    let content_type = ContentType::from_extension(&file.extension().and_then(|ext| ext.to_str()).unwrap_or("none")).unwrap_or(ContentType::Binary);
    if let Ok(contents) = extract_from_exe_zip(&file) {
        debug!("Serving from .zip: {:?}", file);
        return Some((content_type, contents));
    }

    std::fs::File::open(file).and_then(|mut file| {
        let mut buf = String::new();
        file.read_to_string(&mut buf)?;
        debug!("Serving from fs: {:?}", file);
        Ok((content_type, buf))
    }).ok()
}

fn extract_from_exe_zip(file: &Path) -> Result<String, Box<dyn Error>> {
    let mut archive = zip::ZipArchive::new(std::fs::File::open(Path::new(&std::env::args_os().nth(0).ok_or("no exe")?))?)?;
    let mut zipfile = archive.by_name(file.to_str().ok_or("bad file unicode encoding")?)?;
    let mut buf = String::new();
    zipfile.read_to_string(&mut buf)?;
    Ok(buf)
}

#[get("/docs")]
#[tracing::instrument(name="GET /docs")]
async fn docs_index() -> Redirect {
    Redirect::to(uri!(docs("intro")))
}

// This is called with input from our own source code, so if there are errors, be loud.
fn utf8_or_bust(bytes: Vec<u8>, origin: &str) -> String {
    String::from_utf8(bytes).or_else::<(),_>(|e| Ok(format!("# UTF-8 error in {}: {}", origin, e))).unwrap()
}

#[get("/docs/<file..>")]
#[tracing::instrument(name="GET /docs/<file..>")]
async fn docs(file: PathBuf) -> Option<(ContentType, String)> {
    if let Some(extension) = file.extension().and_then(|ext| ext.to_str()) {
        if extension != "md" {
            return file_from_zip_or_fs(&Path::new("docs/").join(file));
        }
    }
    let template = file_from_zip_or_fs(&Path::new("web").join("docs.html.tera")).map(|(_,f)| f).unwrap_or("No template file???".into());
    let contents = file_from_zip_or_fs(&Path::new("docs").join("index.md")).map(|(_,f)| f).unwrap_or("No contents file???".into());
    file_from_zip_or_fs(&Path::new("docs").join(file.with_extension("md")))
                                   .map(|(_,md)| {
                                       use comrak::{parse_document,format_html,markdown_to_html,Arena,ComrakOptions};
                                       let mut options = ComrakOptions::default();
                                       options.extension.header_ids = Some("".to_string());
                                       options.extension.description_lists = true;
                                       options.extension.tasklist = true;

                                       let arena = Arena::new();
                                       let root = parse_document(&arena, &md, &options);

                                       // If the first node is an <h1>, then pull it off and set it to the title so the template can render it nicer.
                                       let mut title = vec![];
                                       let h1 = root.first_child().expect("h1");
                                       if let comrak::nodes::NodeValue::Heading(comrak::nodes::NodeHeading{level:1, setext:_}) = h1.data.borrow().value {
                                           h1.detach();
                                           if let Err(e) = format_html(&h1, &options, &mut title) {
                                               title = format!("Error rendering markdown ast of '{}' into title: {}", file.display(), e).as_bytes().to_vec();
                                           }
                                       }
                                       let mut html = vec![];
                                       if let Err(e) = format_html(&root, &options, &mut html) {
                                           html = format!("Error rendering markdown ast of '{}' into html: {}", file.display(), e).as_bytes().to_vec();
                                       }

                                       trace!("title = {}", String::from_utf8(title.clone()).unwrap());
                                       trace!("html = {}", String::from_utf8(html.clone()).unwrap());

                                       // This used to be real tera, but now we're faking it
                                       (ContentType::HTML, template
                                           .replace("{{ contents | safe }}", &markdown_to_html(&contents, &ComrakOptions::default()))
                                           .replace("{{ title | safe }}",    &utf8_or_bust(title, &file.to_string_lossy()))
                                           .replace("{{ content | safe }}",  &utf8_or_bust(html,   &file.to_string_lossy())))
                                   })
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateRunReq {
    pub user: String,
    pub name: String,
    pub id:   Option<String>,
    pub cmd:  String,
    pub env:  std::vec::Vec<(MaybeUTF8, MaybeUTF8)>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateRunResp {
    pub id: String,
    pub job_id: String,
    pub run_id: String,
}

#[post("/run/create", data="<req>")]
#[tracing::instrument(name="POST /run/create", skip(db,req), fields(req.user=%&req.user,req.name=%&req.name,req.id=req.id.as_deref(),req.cmd=%&req.cmd), ret)]
async fn run_create(db: &State<Db>, req: Json<CreateRunReq>) -> WebResult<Json<CreateRunResp>> {
    let run = db::Run::create(db, &req.user, &req.name, req.id.as_deref(), req.cmd.clone(), req.env.clone()).await?;
    Ok(Json(CreateRunResp { id:format!("{}", run.client_id.unwrap()), job_id: run.job.id, run_id: run.run_id }))
}

pub enum OutKind {
    Stdout, Stderr
}
impl std::fmt::Display for OutKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        match self { OutKind::Stdout => write!(f, "stdout")?,
                     OutKind::Stderr => write!(f, "stderr")? };
        Ok(())
    }
}

#[post("/run/<id>/heartbeat")]
#[tracing::instrument(name="POST /run/<id>/heartbeat", skip(db), ret)]
async fn run_heartbeat(db: &State<Db>, id: u128) -> WebResult<()> {
    let run = db::Run::from_client_id(db, id).await?;
    run.set_heartbeat().await?;
    Ok(())
}


fn short_data(data: &str) -> String {
    format!("[{}]{}...", data.len(), data.get(0..10.min(data.len())).unwrap_or(r"¯\_(ツ)_/¯"))
}

#[post("/run/<id>/stdout", data="<data>")]
#[tracing::instrument(name="POST /run/<id>/stdout", skip(db,data), fields(data=%short_data(&data)))]
async fn run_stdout(db: &State<Db>, id: u128, data: String) -> WebResult<()> {
    run_stdio(db, id, data, OutKind::Stdout).await
}

#[post("/run/<id>/stderr", data="<data>")]
#[tracing::instrument(name="POST /run/<id>/stderr", skip(db,data), fields(data=%short_data(&data)))]
async fn run_stderr(db: &State<Db>, id: u128, data: String) -> WebResult<()> {
    run_stdio(db, id, data, OutKind::Stderr).await
}

async fn run_stdio(db: &State<Db>, id: u128, data: String, _kind: OutKind) -> WebResult<()> {
    let run = db::Run::from_client_id(db, id).await?;
    run.add_stdout(&data)?;
    Ok(())
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq)]
pub enum ExitStatus {
    Exited(i32),
    Signal(i32),
    CoreDump(i32),
    ServerTimeout, // Server didn't get a heartbeat for some period of time
    ClientTimeout, // Client hit timeout waiting for child to complete
}

#[post("/run/<id>/complete", data="<status>")]
#[tracing::instrument(name="POST /run/<id>/complete", skip(db), ret)]
async fn run_complete(db: &State<Db>, id: u128, status: Json<ExitStatus>) -> WebResult<()> {
    let run = db::Run::from_client_id(db, id).await?;
    run.complete(*status).await?;
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Progress {
    pub percent: f32,
    pub eta_seconds: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JobInfo {
    pub id: String,
    pub user: String,
    pub name: String,
    //pub runs: Option<Url>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_run: Option<RunInfo>,
    pub url: String,
    pub runs_url: String,
    pub success_url: String,
    pub settings_url: String,
    pub prune_url: String,
}

impl JobInfo {
    pub async fn from_job(job: &db::Job, latest_run:Option<&db::Run>) -> Result<JobInfo, Box<dyn Error>> {
        Ok(JobInfo{
            id:   job.id.clone(),
            user: job.user.clone(),
            name: job.name.clone(),
            url: uri!(get_job(&job.user, &job.id)).to_string(),
            runs_url: uri!(get_runs(&job.user, &job.id, _, _, _, _)).to_string(),
            success_url: uri!(get_success(&job.user, &job.id, _, _)).to_string(),
            settings_url: uri!(get_job_settings(&job.user, &job.id)).to_string(),
            prune_url: uri!(get_prune(&job.user, &job.id, _)).to_string(),
            latest_run: match latest_run {
                Some(r) => Some(RunInfo::from_run(r).await?),
                None => match job.latest_run().await.map_err(|e| wrap_str(&*e, "latest_run"))? {
                        Some(r) => Some(RunInfo::from_run(&r).await?),
                        None => None,
                },
            }
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RunInfo {
    pub unique_id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url:      Option<String>,
    pub date:     i64,
    pub duration_ms: u64,
    pub id:       String,
    pub status:   Option<ExitStatus>,
    pub progress: Option<Progress>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_len:  Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_url:  Option<String>,
}

impl RunInfo {
    pub async fn from_run(run: &db::Run) -> Result<RunInfo, Box<dyn Error>>  {
        Ok(RunInfo{
            unique_id: run.run_db_id,
            status: run.info().await.map_err(|e| wrap_str(&*e, "info"))?.status,
            progress: run.progress().map_err(|e| wrap_str(&*e, "progress"))?,
            date:     run.date.timestamp_millis(),
            duration_ms: run.duration_ms(),
            id:       run.run_id.clone(),
            log_len:  Some(run.log_len()),
            url:      Some(uri!(get_run(&run.job.user, &run.job.id, &run.run_id, _)).to_string()),
            log_url:  Some(uri!(get_run_log(&run.job.user, &run.job.id, &run.run_id, _, _)).to_string()),
        })
    }
}

#[get("/jobs")]
#[tracing::instrument(name="GET /jobs", skip(db))]
async fn jobs(db: &State<Db>) -> WebResult<Json<Vec<JobInfo>>> {
    use rocket::futures::stream::{self, StreamExt, TryStreamExt};
    let jobs = db::Job::jobs(&db).await.map_err(|e| wrap(&*e, "jobs"))?;
    Ok(Json(stream::iter(jobs.iter())
            .then(async move |job: &db::Job| -> Result<JobInfo, Box<dyn Error>> {
                    Ok(JobInfo::from_job(&job, None).await?)
            }).try_collect().await?))
}

#[get("/runs?<after>&<id>")]
async fn recent_runs(db: &State<Db>, after: Option<u64>, id:Option<Vec<u64>>) -> WebResult<Json<Vec<JobInfo>>> {
    use rocket::futures::stream::{self, StreamExt, TryStreamExt};
    let runs = match (after,id) {
        (Some(after), None) => db::Run::most_recent(&db, after).await?,
        (None, Some(id)) => db::Run::runs_from_ids(&db, &id).await?,
        (_, _) => return Err(Debug(Box::<dyn Error + Send + Sync>::from(format!("Need 'after' xor 'id' parameters")))),
    };
    Ok(Json(stream::iter(runs.iter())
            .then(async move |run: &db::Run| -> Result<JobInfo, Box<dyn Error>> {
                Ok(JobInfo::from_job(&run.job, Some(&run)).await?)
            }).try_collect().await?))
}

#[get("/job/<user>/<job_id>")]
async fn get_job(db: &State<Db>, user: &str, job_id: &str) -> WebResult<Json<JobInfo>> {
    let job = db::Job::new(&db, user, job_id).await.map_err(|e| wrap(&*e, "db::Job"))?;
    Ok(Json(JobInfo::from_job(&job, None).await?))
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RunInfoFull {
    #[serde(flatten)]
    pub run_info: RunInfo,
    pub cmd:      String,
    pub env:      Vec<(MaybeUTF8, MaybeUTF8)>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log:      Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seek:     Option<u64>,
}

#[get("/job/<user>/<job_id>/run?<num>&<before>&<after>&<id>")]
#[tracing::instrument(name="GET /job/<user>/<job_id>/run", skip(db))]
async fn get_runs(db: &State<Db>, user: &str, job_id: &str, num: Option<u32>, before: Option<u64>, after: Option<u64>, id:Option<Vec<&str>>) -> WebResult<Json<Vec<RunInfo>>> {
    let job = db::Job::new(&db, user, job_id).await.map_err(|e| wrap(&*e, "db::Job"))?;
    use rocket::futures::stream::{self, StreamExt, TryStreamExt};
    let jobs = match id {
        Some(id) if id.len() > 0  => job.runs_from_ids(&id).await?,
        _                         => job.runs(num, before, after).await?
    };
    debug!("Got {} runs for {}", jobs.len(), job_id);
    Ok(Json(stream::iter(jobs.into_iter())
            .then(async move |run| -> Result<RunInfo, Box<dyn Error>> {
                Ok(RunInfo::from_run(&run).await?)
            }).try_collect().await?))
}

#[get("/job/<user>/<job_id>/run/<run_id>?<seek>")]
#[tracing::instrument(name="GET /job/<user>/<job_id>/run/<run_id>?<seek>", skip(db))]
async fn get_run(db: &State<Db>, user: &str, job_id: &str, run_id: &str, seek: Option<u64>) -> WebResult<Json<RunInfoFull>> {
    //Err(Debug(format!("This is a test")))?;
    let job = db::Job::new(&db, user, job_id).await.map_err(|e| wrap(&*e, "db::Job"))?;
    let run = job.run(run_id).await.map_err(|e| wrap(&*e, "run"))?;
    let info = run.info().await.map_err(|e| wrap(&*e, "info"))?;
    let (log, log_len, log_url) = match run.log_len() {
        0                     => (None, None, None),
        log_len @ 1...300_000 => { // If it's short enough, give the log back inline
            use tokio::io::AsyncReadExt;
            let Some(mut log_file) = run.log_file().await.map_err(|e| wrap(&*e, "log"))? else { Err(Debug(format!("log_len() was {} but log() said None!", log_len).into()))? };
            let (_total, length) = seek_and_limit(&mut log_file, seek, None).await.map_err(|e| wrap(&*e, "log seek"))?;
            let mut log = String::with_capacity(length as usize);
            log_file.read_to_string(&mut log).await.map_err(|e| wrap(&e, "log read"))?;
            (Some(log), Some(log_len), Some(uri!(get_run_log(user, job_id, run_id, _, _)).to_string()))
        },
        log_len               => (None, Some(log_len), Some(uri!(get_run_log(user, job_id, run_id, _, _)).to_string())),
    };
    Ok(Json(RunInfoFull{
        run_info: RunInfo {
            unique_id: run.run_db_id,
            status:   info.status,
            progress: run.progress().map_err(|e| wrap(&*e, "progress"))?,
            date:     run.date.timestamp_millis(),
            duration_ms: run.duration_ms(),
            id:       run.run_id.clone(),
            url:      None,
            log_len:  log_len,
            log_url:  log_url,
        },
        cmd:      info.cmd,
        env:      info.env,
        log:      log,
        seek:     seek,
    }))
}

struct LogStreamer {
    log: tokio::fs::File,
    len: u64,
    total: u64,
}

impl<'r> Responder<'r, 'static> for LogStreamer {
    fn respond_to(self, _req: &'r Request<'_>) -> rocket::response::Result<'static> {
        Response::build()
            .header(ContentType::Plain)
            .raw_header("x-log-length", format!("{}", self.total))
            .sized_body(self.len as usize, self.log)
            .ok()
    }
}

#[get("/job/<user>/<job_id>/run/<run_id>/log?<seek>&<limit>")]
#[tracing::instrument(name="GET /job/<user>/<job_id>/run/<run_id>/log?<seek>&<limit>", skip(db))]
async fn get_run_log(db: &State<Db>, user: &str, job_id: &str, run_id: &str, seek: Option<u64>, limit: Option<i64>) -> WebResult<Option<LogStreamer>> {
    let job = db::Job::new(&db, user, job_id).await.map_err(|e| wrap(&*e, "db::Job"))?;
    let run = job.run(run_id).await.map_err(|e| wrap(&*e, "run"))?;
    let Some(mut log) = run.log_file().await.map_err(|e| wrap(&*e, "log"))? else {
        return Ok(None);
    };
    let (total, len) = seek_and_limit(&mut log, seek, limit).await.map_err(|e| wrap(&*e, "seek_and_limit"))?;
    Ok(Some(LogStreamer { log, total, len }))
}

pub (crate) async fn seek_and_limit(f: &mut tokio::fs::File, seek: Option<u64>, limit: Option<i64>) -> Result<(u64, u64), Box<dyn Error>> {
    use tokio::io::AsyncSeekExt;
    let total = f.metadata().await?.len();
    let (seek, len) = apply_limit(total, seek, limit);
    f.seek(std::io::SeekFrom::Start(seek)).await?;
    Ok((total, len))
}

// Given the length of a file, an optional seek and an optional signed limit:
// return the computed seek and length of read as a tuple: (seek, len).
// positive limit means from start--seek won't be changed
// negative limit means from the end--seek will adjusted such that seek+min(len,limit) == eof.
// length will always be clamped to limit
pub fn apply_limit(len: u64, seek: Option<u64>, limit: Option<i64>) -> (u64, u64) {
    let limit = limit.unwrap_or(i64::MAX);
    let seek = seek.unwrap_or(0);
    match (len, seek, limit < 0, limit.abs_diff(0)) {
        (len, seek, false, limit) => (seek,                                limit.min(len.saturating_sub(seek))), // Positive limit is from the start (which is seek)
        (len, seek, true,  limit) => (seek.max(len.saturating_sub(limit)), limit.min(len.saturating_sub(seek))), // negative limit is from the end
    }
}
#[cfg(test)] #[test] fn test_apply_limit() {
    assert_eq!(apply_limit(10, None,    None),      (0, 10));
    assert_eq!(apply_limit(10, Some(5), None),      (5,  5));
    assert_eq!(apply_limit(10, None,    Some(3)),   (0,  3));
    assert_eq!(apply_limit(10, None,    Some(-3)),  (7,  3));
    assert_eq!(apply_limit(10, None,    Some(13)),  (0, 10));
    assert_eq!(apply_limit(10, None,    Some(-13)), (0, 10));
    assert_eq!(apply_limit(10, Some(5), Some(3)),   (5,  3));
    assert_eq!(apply_limit(10, Some(5), Some(-3)),  (7,  3));
    assert_eq!(apply_limit(10, Some(5), Some(17)),  (5,  5));
    assert_eq!(apply_limit(10, Some(5), Some(-17)), (5,  5));
}


#[get("/job/<user>/<job_id>/success?<before>&<after>")]
async fn get_success(db: &State<Db>, user: &str, job_id: &str, before: Option<u64>, after: Option<u64>) -> WebResult<Json<Vec<(i64,Option<bool>)>>> {
    let job = db::Job::new(&db, user, job_id).await.map_err(|e| wrap(&*e, "db::Job"))?;
    Ok(Json(job.successes(before, after).await?))
}


#[get("/job/<user>/<job_id>/settings")]
async fn get_job_settings(db: &State<Db>, user: &str, job_id: &str) -> WebResult<Json<db::JobSettings>> {
    let job = db::Job::new(&db, user, job_id).await.map_err(|e| wrap(&*e, "db::Job"))?;
    Ok(Json(job.settings))
}

#[put("/job/<user>/<job_id>/settings", data="<settings>")]
async fn put_job_settings(db: &State<Db>, user: &str, job_id: &str, settings: Json<db::JobSettings>) -> WebResult<()> {
    let job = db::Job::new(&db, user, job_id).await.map_err(|e| wrap(&*e, "db::Job"))?;
    job.update_settings(&settings).await?;
    Ok(())
}


#[derive(Debug, Serialize, Deserialize)]
struct PruneResult {
    pruned: Vec<db::Pruned>,
    stats: db::PruneStats,
}

#[get("/job/<user>/<job_id>/prune?<settings>")]
async fn get_prune(db: &State<Db>, user: &str, job_id: &str, settings: Option<Json<db::RetentionSettings>>) -> WebResult<Json<PruneResult>> {
    let job = db::Job::new(&db, user, job_id).await.map_err(|e| wrap(&*e, "db::Job"))?;
    let mut stats = db::PruneStats::default();
    let pruned = job.prune_dry_run(Some(&mut stats), settings.map(|s| s.into_inner())).await?;
    Ok(Json(PruneResult { pruned, stats }))
}

#[post("/job/<user>/<job_id>/prune")]
async fn post_prune(db: &State<Db>, user: &str, job_id: &str) -> WebResult<Json<PruneResult>> {
    let job = db::Job::new(&db, user, job_id).await.map_err(|e| wrap(&*e, "db::Job"))?;
    let mut stats = db::PruneStats::default();
    let pruned = job.prune(Some(&mut stats)).await?;
    Ok(Json(PruneResult { pruned, stats }))
}

#[derive(Debug, Serialize, Deserialize)]
struct Settings {
    retention: db::RetentionSettings,
}

#[get("/settings")]
async fn get_settings(db: &State<Db>) -> WebResult<Json<Settings>> {
    let settings = db::Settings::load(db).await?;
    Ok(Json(Settings { retention: settings.retention }))
}

#[put("/settings", data="<new_settings>")]
async fn put_settings(db: &State<Db>, new_settings: Json<Settings>) -> WebResult<()> {
    let mut settings = db::Settings::load(db).await?;
    settings.set_retention(new_settings.retention).await?;
    Ok(())
}

#[post("/shutdown")]
#[tracing::instrument(name="POST /shutdown", skip_all)]
fn shutdown(shutdown: rocket::Shutdown) -> &'static str {
    shutdown.notify();
    "Shutting down..."
}

pub async fn serve(port: u16, db: &Db, enable_shutdown: bool) -> Result<(), Box<dyn std::error::Error>> {
    let figment = figment::Figment::from(rocket::Config::figment())
        .merge(("address", "0.0.0.0".parse::<std::net::IpAddr>().unwrap()))
        .merge(("port", port))
        .merge(figment::providers::Env::prefixed("SYNCRON_").global())
        .select(figment::Profile::from_env_or("APP_PROFILE", "default"));
    let mut routes = routes![index, files, docs_index, docs,
                             // client endpoints
                             run_create, run_heartbeat, run_stdout, run_stderr, run_complete,
                             // web app endpoints
                             jobs, recent_runs, get_job, get_runs, get_run, get_run_log, get_success,
                             get_job_settings, put_job_settings, get_prune, post_prune, get_settings, put_settings];
    if enable_shutdown { routes.append(&mut routes![shutdown]) }
    let _rocket = rocket::custom(figment)
        .mount("/", routes)
        .manage(db.clone())
        .launch().await?;
    Ok(())
}
