// Copyright © 2022 David Caldwell <david@porkrind.org>

use std::path::{Path,PathBuf};

use sqlx::migrate::Migrator;
pub static MIGRATOR: Migrator = sqlx::migrate!(); // defaults to "./migrations"

#[derive(Debug, Clone)]
pub struct Db {
    db_path: PathBuf,
    sql: sqlx::SqlitePool,
}

impl Db {
    pub fn new(sql: &sqlx::SqlitePool,
               db_path: &Path)      -> Db { Db{ db_path:db_path.into(),
                                                sql: sql.clone(), } }
    pub fn sql(&self)               -> &sqlx::SqlitePool { &self.sql }
    pub fn jobs_path(&self)         -> PathBuf { self.db_path.join("jobs") }
}
