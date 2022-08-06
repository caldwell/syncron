// Copyright © 2022 David Caldwell <david@porkrind.org>

use std::error::Error;
use std::io::Read;
use std::path::{Path,PathBuf};

use rocket::http::ContentType;
use rocket::response::{Debug,Redirect};
use rocket::serde::{Serialize, Deserialize, json::Json};
use rocket::State;
use rocket::fairing::AdHoc;

use crate::db;
use crate::db::Db;
use crate::maybe_utf8::MaybeUTF8;
use crate::{wrap,wrap_str};

use rocket_db_pools::{sqlx,Database};
#[derive(Database)]
#[database("sqldb")]
struct SQLDb(sqlx::SqlitePool);

#[derive(Debug, Deserialize, Serialize)]
#[serde(crate = "rocket::serde")]
struct Config {
    db_path: PathBuf,
}

impl Default for Config {
    fn default() -> Config {
        Config { db_path: "./db".into(), }
    }
}

type WebResult<T, E = Debug<Box<dyn Error>>> = std::result::Result<T, E>; // What is this magic??

#[get("/")]
async fn index() -> Option<(ContentType, String)> {
    files("index.html".into()).await
}

#[get("/<file..>")]
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
async fn docs_index() -> Redirect {
    Redirect::to(uri!(docs("intro")))
}

// This is called with input from our own source code, so if there are errors, be loud.
fn utf8_or_bust(bytes: Vec<u8>, origin: &str) -> String {
    String::from_utf8(bytes).or_else::<(),_>(|e| Ok(format!("# UTF-8 error in {}: {}", origin, e))).unwrap()
}

#[get("/docs/<file..>")]
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
async fn run_create(conf: &State<Config>, sql: &SQLDb, req: Json<CreateRunReq>) -> WebResult<Json<CreateRunResp>> {
    let run = db::Run::create(&sql, conf.db_path.clone(), &req.user, &req.name, req.id.as_deref(), req.cmd.clone(), req.env.clone()).await?;
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
async fn run_heartbeat(conf: &State<Config>, sql: &SQLDb, id: u128) -> WebResult<()> {
    let run = db::Run::from_client_id(&sql, conf.db_path.clone().into(), id).await?;
    run.set_heartbeat().await?;
    Ok(())
}

#[post("/run/<id>/stdout", data="<data>")]
async fn run_stdout(conf: &State<Config>, sql: &SQLDb, id: u128, data: String) -> WebResult<()> {
    run_stdio(conf, sql, id, data, OutKind::Stdout).await
}

#[post("/run/<id>/stderr", data="<data>")]
async fn run_stderr(conf: &State<Config>, sql: &SQLDb, id: u128, data: String) -> WebResult<()> {
    run_stdio(conf, sql, id, data, OutKind::Stderr).await
}

async fn run_stdio(conf: &State<Config>, sql: &SQLDb, id: u128, data: String, _kind: OutKind) -> WebResult<()> {
    let run = db::Run::from_client_id(&sql, conf.db_path.clone().into(), id).await?;
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
async fn run_complete(conf: &State<Config>, sql: &SQLDb, id: u128, status: Json<ExitStatus>) -> WebResult<()> {
    let run = db::Run::from_client_id(sql, conf.db_path.clone().into(), id).await?;
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
    pub latest_run: RunInfo,
    pub runs_url: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RunInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url:      Option<String>,
    pub date:     i64,
    pub id:       String,
    pub status:   Option<ExitStatus>,
    pub progress: Option<Progress>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_len:  Option<u64>,
}

#[get("/jobs")]
async fn jobs(conf: &State<Config>, sql: &SQLDb) -> WebResult<Json<Vec<JobInfo>>> {
    let db = Db::new(&sql, &conf.db_path.clone());
    use rocket::futures::stream::{self, StreamExt};
    let jobs = db::Job::jobs(&db).await.map_err(|e| wrap(&*e, "jobs"))?;
    Ok(Json(stream::iter(jobs.iter())
            .then(async move |job| -> Result<JobInfo, String> {
                (async move |job: db::Job| -> Result<JobInfo, String> {
                    let latest_run = job.latest_run().await.map_err(|e| wrap_str(&*e, "latest_run"))?.unwrap();
                    Ok(JobInfo{ id:   job.id.clone(),
                                user: job.user.clone(),
                                name: job.name.clone(),
                                runs_url: uri!(get_runs(&job.user, &job.id)).to_string(),
                                latest_run: RunInfo{
                                    status: latest_run.info().await.map_err(|e| wrap_str(&*e, "info"))?.status,
                                    progress: latest_run.progress().map_err(|e| wrap_str(&*e, "progress"))?,
                                    date:     latest_run.date.timestamp(),
                                    id:       latest_run.run_id.clone(),
                                    log_len:  Some(latest_run.log_len()),
                                    url:      Some(uri!(get_run(&job.user, &job.id, latest_run.run_id, Option::<u64>::None)).to_string()),
                                },
                    })
                })(job.clone()).await.map_err(|e| format!("{}: {}", e, &job.id))
            }).filter_map(async move |ji| {
                if let Err(ref e) = ji { warn!("skipping job due to error: {}", e) }
                ji.ok()
            }).collect().await))
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RunInfoFull {
    #[serde(flatten)]
    pub run_info: RunInfo,
    pub cmd:      String,
    pub env:      Vec<(MaybeUTF8, MaybeUTF8)>,
    pub log:      Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seek:     Option<u64>,
}

#[get("/job/<user>/<job_id>/run")]
async fn get_runs(conf: &State<Config>, sql: &SQLDb, user: &str, job_id: &str) -> WebResult<Json<Vec<RunInfo>>> {
    let db = Db::new(&sql, &conf.db_path.clone());
    let job = db::Job::new(&db, user, job_id).await.map_err(|e| wrap(&*e, "db::Job"))?;
    use rocket::futures::stream::{self, StreamExt};
    let jobs = job.runs().await?;
    debug!("Got {} runs for {}", jobs.len(), job_id);
    Ok(Json(stream::iter(jobs.into_iter()).then(async move |run| -> Result<RunInfo, String> {
        let info = run.info().await.map_err(|e| wrap_str(&*e, "info"))?;
        Ok(RunInfo{
            status:   info.status,
            progress: run.progress().map_err(|e| wrap_str(&*e, "progress"))?,
            date:     run.date.timestamp(),
            id:       run.run_id.clone(),
            log_len:  Some(run.log_len()),
            url:      Some(uri!(get_run(&run.job.user, &run.job.id, run.run_id, Option::<u64>::None)).to_string()),
        })
    }).filter_map(async move |ri| ri.ok()).collect().await))
}

#[get("/job/<user>/<job_id>/run/<run_id>?<seek>")]
async fn get_run(conf: &State<Config>, sql: &SQLDb, user: &str, job_id: &str, run_id: &str, seek: Option<u64>) -> WebResult<Json<RunInfoFull>> {
    let db = Db::new(&sql, &conf.db_path.clone());
    //Err(Debug(format!("This is a test")))?;
    let job = db::Job::new(&db, user, job_id).await.map_err(|e| wrap(&*e, "db::Job"))?;
    let run = job.run(run_id).await.map_err(|e| wrap(&*e, "run"))?;
    let info = run.info().await.map_err(|e| wrap(&*e, "info"))?;
    Ok(Json(RunInfoFull{
        run_info: RunInfo {
            status:   info.status,
            progress: run.progress().map_err(|e| wrap(&*e, "progress"))?,
            date:     run.date.timestamp(),
            id:       run.run_id.clone(),
            url:      None,
            log_len:  None,
        },
        cmd:      info.cmd,
        env:      info.env,
        log:      run.log(seek).map_err(|e| wrap(&*e, "log"))?,
        seek:     seek,
    }))
}

#[post("/shutdown")]
fn shutdown(shutdown: rocket::Shutdown) -> &'static str {
    shutdown.notify();
    "Shutting down..."
}

async fn run_migrations(rocket: rocket::Rocket<rocket::Build>) -> rocket::fairing::Result {
    match SQLDb::fetch(&rocket) {
        Some(db) => match crate::db::MIGRATOR.run(&**db).await {
            Ok(_) => Ok(rocket),
            Err(e) => {
                error!("Failed to initialize SQLx database: {}", e);
                Err(rocket)
            }
        }
        None => Err(rocket),
    }
}

pub async fn serve(port: u16, db_path: PathBuf, enable_shutdown: bool) -> Result<(), Box<dyn std::error::Error>> {
    let figment = figment::Figment::from(rocket::Config::figment())
        .merge(("address", "0.0.0.0".parse::<std::net::IpAddr>().unwrap()))
        .merge(("port", port))
        .merge(figment::providers::Env::prefixed("SYNCRON_").global())
        .select(figment::Profile::from_env_or("APP_PROFILE", "default"))
        .merge(("databases.sqldb.url", db_path.join("syncron.sqlite3")))
        .merge(("db_path", db_path));
    let mut routes = routes![index, files, docs_index, docs,
                             run_create, run_heartbeat, run_stdout, run_stderr, run_complete, jobs, get_runs, get_run];
    if enable_shutdown { routes.append(&mut routes![shutdown]) }
    let _rocket = rocket::custom(figment)
        .mount("/", routes)
        .attach(AdHoc::config::<Config>())
        .attach(SQLDb::init())
        .attach(AdHoc::try_on_ignite("SQLx Migrations", run_migrations))
        .launch().await?;
    Ok(())
}


