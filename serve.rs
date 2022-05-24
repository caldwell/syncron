// Copyright © 2022 David Caldwell <david@porkrind.org>

use std::error::Error;
use std::path::{Path,PathBuf};

use rocket::fs::NamedFile;
use rocket::response::{Debug,Redirect};
use rocket::serde::{Serialize, Deserialize, json::Json};
use rocket::State;
use rocket::fairing::AdHoc;
use rocket_dyn_templates::{Template,context};

use crate::job::{ServerJob,ServerRun,ServerRunInfo};
use crate::db::Db;
use crate::maybe_utf8::MaybeUTF8;

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
async fn index() -> Option<NamedFile> {
    NamedFile::open(Path::new("web/index.html")).await.ok()
}

#[get("/<file..>")]
async fn files(file: PathBuf) -> Option<NamedFile> {
    NamedFile::open(Path::new("web/").join(file)).await.ok()
}

#[get("/docs")]
async fn docs_index() -> Redirect {
    Redirect::to(uri!(docs("intro")))
}

fn utf8_or_bust(bytes: Vec<u8>, origin: &str) -> String {
    String::from_utf8(bytes).or_else::<(),_>(|e| Ok(format!("# UTF-8 error in {}: {}", origin, e))).unwrap()
}

#[get("/docs/<file..>")]
async fn docs(file: PathBuf) -> Option<Template> {
    let contents = std::fs::read(Path::new("docs").join("index.md")).ok();
    std::fs::read(Path::new("docs").join(file.with_extension("md"))).ok()
                                   .map(|md| {
                                       use comrak::{parse_document,format_html,markdown_to_html,Arena,ComrakOptions};
                                       let mut options = ComrakOptions::default();
                                       options.extension.header_ids = Some("".to_string());

                                       let arena = Arena::new();
                                       let root = parse_document(&arena, &utf8_or_bust(md, &file.to_string_lossy()), &options);

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

                                       Template::render("docs", context! {
                                           title: utf8_or_bust(title, &file.to_string_lossy()),
                                           content: utf8_or_bust(html, &file.to_string_lossy()),
                                           contents: markdown_to_html(&utf8_or_bust(contents.unwrap_or("No contents file???".into()), "docs/index.md"),
                                                                              &ComrakOptions::default()),
                                       })
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
async fn run_create(conf: &State<Config>, req: Json<CreateRunReq>) -> WebResult<Json<CreateRunResp>> {
    let run = ServerRun::create(conf.db_path.clone(), &req.user, &req.name, req.id.as_deref())?;
    run.set_info(&ServerRunInfo{
        cmd:    req.cmd.clone(),
        env:    req.env.clone(),
        end:    None,
        status: None,
    })?;
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

#[post("/run/<id>/stdout", data="<data>")]
async fn run_stdout(conf: &State<Config>, id: u128, data: String) -> WebResult<()> {
    run_stdio(conf, id, data, OutKind::Stdout).await
}

#[post("/run/<id>/stderr", data="<data>")]
async fn run_stderr(conf: &State<Config>, id: u128, data: String) -> WebResult<()> {
    run_stdio(conf, id, data, OutKind::Stderr).await
}

async fn run_stdio(conf: &State<Config>, id: u128, data: String, _kind: OutKind) -> WebResult<()> {
    let run = ServerRun::from_client_id(conf.db_path.clone().into(), id)?;
    run.add_stdout(&data)?;
    Ok(())
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub enum ExitStatus {
    Exited(i32),
    Signal(i32),
    CoreDump(i32),
}

#[post("/run/<id>/complete", data="<status>")]
async fn run_complete(conf: &State<Config>, id: u128, status: Json<ExitStatus>) -> WebResult<()> {
    let run = ServerRun::from_client_id(conf.db_path.clone().into(), id)?;
    run.complete(*status)?;
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
async fn jobs(conf: &State<Config>) -> WebResult<Json<Vec<JobInfo>>> {
    let db = Db::new(&conf.db_path.clone());
    Ok(Json(db.jobs()?.iter()
            .map(|job| -> Result<JobInfo, Box<dyn Error>> {
                (|| -> Result<JobInfo, Box<dyn Error>> {
                    let latest_run = job.latest_run()?.unwrap();
                    Ok(JobInfo{ id:   job.id.clone(),
                                user: job.user.clone(),
                                name: job.name()?.clone(),
                                runs_url: uri!(get_runs(&job.user, &job.id)).to_string(),
                                latest_run: RunInfo{
                                    status: latest_run.info().map_err(|e| wrap(&*e, "info"))?.status,
                                    progress: latest_run.progress()?,
                                    date:     latest_run.date.timestamp(),
                                    id:       latest_run.run_id.clone(),
                                    log_len:  Some(latest_run.log_len()),
                                    url:      Some(uri!(get_run(&job.user, &job.id, latest_run.run_id, Option::<u64>::None)).to_string()),
                                },
                    })
                })().map_err(|e| wrap(&*e, &job.id))
            }).filter_map(|ji| {
                if let Err(ref e) = ji { warn!("skipping job due to error: {}", e) }
                ji.ok()
            }).collect()))
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

fn wrap<E: Error>(e: E, s: &str) -> Box<dyn Error> {
    Box::<dyn Error>::from(format!("{}: {:?}", s, e))
}

#[get("/job/<user>/<job_id>/run")]
async fn get_runs(conf: &State<Config>, user: &str, job_id: &str) -> WebResult<Json<Vec<RunInfo>>> {
    let db = Db::new(&conf.db_path.clone());
    let job = ServerJob::new(&db, user, job_id).map_err(|e| wrap(&*e, "ServerJob"))?;
    Ok(Json(job.runs()?.into_iter().map(|run| -> Result<RunInfo, Box<dyn Error>> {
        let info = run.info().map_err(|e| wrap(&*e, "info"))?;
        Ok(RunInfo{
            status:   info.status,
            progress: run.progress().map_err(|e| wrap(&*e, "progress"))?,
            date:     run.date.timestamp(),
            id:       run.run_id.clone(),
            log_len:  Some(run.log_len()),
            url:      Some(uri!(get_run(&job.user, &job.id, run.run_id, Option::<u64>::None)).to_string()),
        })
    }).filter_map(|ri| ri.ok()).collect()))
}

#[get("/job/<user>/<job_id>/run/<run_id>?<seek>")]
async fn get_run(conf: &State<Config>, user: &str, job_id: &str, run_id: &str, seek: Option<u64>) -> WebResult<Json<RunInfoFull>> {
    let db = Db::new(&conf.db_path.clone());
    //Err(Debug(format!("This is a test")))?;
    let job = ServerJob::new(&db, user, job_id).map_err(|e| wrap(&*e, "ServerJob"))?;
    let run = job.run(run_id).map_err(|e| wrap(&*e, "run"))?;
    let info = run.info().map_err(|e| wrap(&*e, "info"))?;
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

pub async fn serve(port: u16, db_path: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let figment = figment::Figment::from(rocket::Config::figment())
        .merge(("address", "0.0.0.0".parse::<std::net::IpAddr>().unwrap()))
        .merge(("port", port))
        .merge(figment::providers::Env::prefixed("SYNCRON_").global())
        .select(figment::Profile::from_env_or("APP_PROFILE", "default"))
        .merge(("db_path", db_path))
        .merge(("template_dir", "web"));
    let _rocket = rocket::custom(figment)
        .mount("/", routes![index, files, docs_index, docs,
                            run_create, run_stdout, run_stderr, run_complete, jobs, get_runs, get_run])
        .attach(Template::fairing())
        .attach(AdHoc::config::<Config>())
        .launch().await?;
    Ok(())
}


