// Copyright © 2022 David Caldwell <david@porkrind.org>

use std::error::Error;
use std::path::{Path,PathBuf};

use crate::job::{ServerJob};

#[derive(Debug, PartialEq, Clone)]
pub struct Db {
    db: PathBuf,
}

impl Db {
    pub fn new(db: &Path)           -> Db { Db{ db:db.into() } }
    pub fn ids_path(&self)          -> PathBuf { self.db.join("ids") }
    pub fn id_path(&self, id: u128) -> PathBuf { self.db.join("ids").join(&format!("{}", id)) }
    pub fn jobs_path(&self)         -> PathBuf { self.db.join("jobs") }

    pub fn dirs(&self, dir: &Path) -> Result<Vec<String>, Box<dyn Error>> {
        if !dir.exists() { return Ok(vec![]); }
        Ok(std::fs::read_dir(dir)?
           .filter_map(|entry| entry.ok())
           .filter(|entry| match entry.metadata() { Ok(m) => m.file_type().is_dir(), _ => false })
           .map(|entry| entry.file_name().to_string_lossy().into())
           .collect())
    }

    pub fn users(&self) -> Result<Vec<String>, Box<dyn Error>> {
        self.dirs(&self.jobs_path())
    }

    pub fn jobs_for_user(&self, user: &str) -> Result<Vec<String>, Box<dyn Error>> {
        self.dirs(&self.jobs_path().join(user))
    }

    pub fn jobs(&self) -> Result<Vec<ServerJob>, Box<dyn Error>> {
        let mut jobs: Vec<ServerJob> = vec![];
        for user in self.users()? {
            jobs.append(&mut self.jobs_for_user(&user)?.iter()
                        .map(|id| ServerJob::new(&self, &user.clone(), &id.to_string()))
                        .filter_map(|j| j.ok())
                        .collect())
        }
        Ok(jobs)
    }
}
