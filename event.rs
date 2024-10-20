// Copyright © 2024 David Caldwell <david@porkrind.org>

use std::sync::Arc;

use serde::Serialize;
use tokio::sync::{mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender}, Mutex};

use crate::db;

#[derive(Clone, Debug, Serialize)]
pub struct Event {
    topic: String,
    #[serde(flatten)]
    detail: EventDetail,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventDetail {
    JobCreate(crate::serve::JobInfo),
    JobUpdate(crate::serve::JobInfo),
    JobDelete,
    RunCreate(crate::serve::RunInfo),
    RunUpdate(crate::serve::RunInfo),
    RunDelete { reason: String },
    RunLogAppend { chunk: String },
}

impl Event {
    pub fn job_create(job: &db::Job) -> Self {
        Event { topic: format!("job"),
                detail: EventDetail::JobCreate(job.into()) }
    }
    pub fn job_update(job: &db::Job) -> Self {
        Event { topic: format!("job/{}/{}", job.user, job.id),
                detail: EventDetail::JobUpdate(job.into()) }
    }
    pub fn job_delete(job: &db::Job) -> Self {
        Event { topic: format!("job/{}/{}", job.user, job.id),
                detail: EventDetail::JobDelete }
    }

    pub fn run_create(run: &db::Run) -> Self {
        Event { topic: format!("job/{}/{}/run", run.job.user, run.job.id),
                detail: EventDetail::RunCreate(run.into()) }
    }
    pub fn run_update(run: &db::Run) -> Self {
        Event { topic: format!("job/{}/{}/run/{}", run.job.user, run.job.id, run.run_id),
                detail: EventDetail::RunUpdate(run.into()) }
    }
    pub fn run_delete(run: &db::Run, reason: &str) -> Self {
        Event { topic: format!("job/{}/{}/run/{}", run.job.user, run.job.id, run.run_id),
                detail: EventDetail::RunDelete { reason: reason.to_owned() } }
    }

    pub fn run_log_append(run: &db::Run, chunk: &str) -> Self {
        Event { topic: format!("job/{}/{}/run/{}/log", run.job.user, run.job.id, run.run_id),
                detail: EventDetail::RunLogAppend { chunk: chunk.to_owned() } }
    }
}

#[derive(Clone, Debug)]
pub struct Broker {
    subs: Arc<Mutex<Vec<(Filter, UnboundedSender<Event>)>>>,
}

impl Broker {
    pub fn new() -> Broker {
        Broker {
            subs: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub async fn subscribe(&self, topic_filters: &[&str]) -> Result<UnboundedReceiver<Event>, Box<dyn std::error::Error>> {
        let (tx, rx) = unbounded_channel();
        let mut subs = self.subs.lock().await;
        for topic_filter in topic_filters.into_iter() {
            subs.push((Filter::new(topic_filter)?, tx.clone()));
        }
        Ok(rx)
    }

    pub async fn send(&self, event: Event) {
        let mut subs = self.subs.lock().await;
        for sub in subs.iter() {
            if sub.0 == &event.topic {
                _ = sub.1.send(event.clone()).is_ok(); // We'll deal with closed ones in a sec
            }
        }
        subs.retain(|s| !s.1.is_closed());
    }
}

#[derive(Clone, Debug)]
pub struct Filter(Vec<Option<String>>);

impl Filter {
    fn new(filter: &str) -> Result<Filter, Box<dyn std::error::Error>> {
        let mut f = Filter(filter.split('/').map(|s| Some(s.to_owned())).collect());
        if !matches!(f.0.last(), Some(Some(ref c)) if c == "#") { f.0.push(None) } // Guard on the end for non matchers
        if let Some(e) = f.0.iter().enumerate().find_map(|(i, c)| {
            if let Some(ref c) = c {
                if c.len() > 1 && (c.contains('#')) { return Some(Err(format!("Invalid filter {filter:?}: '#' has to be by itself"))) }
                if c.len() > 1 && (c.contains('+')) { return Some(Err(format!("Invalid filter {filter:?}: '+' has to be by itself"))) }
                if c.as_str() == "#" && i != f.0.len()-1 { return Some(Err(format!("Invalid filter {filter:?}: '+' has to be by itself"))) }
            }
            None
        }) { e? }
        Ok(f)
    }

    fn matches(&self, topic: &str) -> bool {
        let filter_iter = self.0.iter().chain(std::iter::repeat(self.0.last().unwrap()));
        let topic_iter = topic.split('/').map(|c| Some(c)).chain([None]);
        filter_iter.zip(topic_iter).fold(true, |acc, (filter, topic)| acc && match (filter.as_deref(), topic) {
            (Some("+"), Some(_)) => true,
            (Some("#"), _) => true,
            (Some(f), Some(s)) => f == s,
            (None, None) => true, // means they ended at the same place
            _ => false,
        })
    }
}

impl std::cmp::PartialEq<&str> for Filter {
    fn eq(&self, other: &&str) -> bool {
        self.matches(*other)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_filter() {
        assert_eq!(true,  Filter::new("sport").unwrap().matches("sport"));
        assert_eq!(false, Filter::new("sport").unwrap().matches("sport/tennis"));
        assert_eq!(false, Filter::new("sport/tennis").unwrap().matches("sport"));
        assert_eq!(true,  Filter::new("sport").unwrap() == "sport");
    }

    #[test]
    fn test_filter_wildcard() {
        assert_eq!(true,  Filter::new("+").unwrap().matches("something"));
        assert_eq!(false, Filter::new("+").unwrap().matches("something/else"));
        assert_eq!(true,  Filter::new("sport/tennis/+").unwrap().matches("sport/tennis/player1"));
        assert_eq!(true,  Filter::new("sport/tennis/+").unwrap().matches("sport/tennis/player2"));
        assert_eq!(false, Filter::new("sport/tennis/+").unwrap().matches("sport/tennis/player1/ranking"));
        assert_eq!(true,  Filter::new("sport/tennis/+").unwrap().matches("sport/tennis/"));
        assert_eq!(true,  Filter::new("+/+").unwrap() == "/finance");
        assert_eq!(true,  Filter::new("/+").unwrap()  == "/finance");
        assert_eq!(false, Filter::new("+").unwrap()   == "/finance");
        assert_eq!(true,  Filter::new("sport/+/player1").unwrap().matches("sport/tennis/player1"));
        assert_eq!(true,  Filter::new("sport/+/player1").unwrap().matches("sport/football/player1"));
        assert_eq!(false, Filter::new("sport/+/player1").unwrap().matches("sport/football/player2"));
    }

    #[test]
    fn test_filter_multilevel_wildcard() {
        assert_eq!(true,  Filter::new("sport/tennis/player1/#").unwrap().matches("sport/tennis/player1"));
        assert_eq!(true,  Filter::new("sport/tennis/player1/#").unwrap().matches("sport/tennis/player1/ranking"));
        assert_eq!(true,  Filter::new("sport/tennis/player1/#").unwrap().matches("sport/tennis/player1/score/wimbledon"));
    }

    #[test]
    fn test_filter_invalid() {
        assert!(matches!(Filter::new("sport+"), Err(_)));
        assert!(matches!(Filter::new("sp+rts"), Err(_)));
        assert!(matches!(Filter::new("sp#rt/tennis"), Err(_)));
        assert!(matches!(Filter::new("sport/tennis#"), Err(_)));
        assert!(matches!(Filter::new("sport/tennis/#/ranking"), Err(_)));
    }
}


