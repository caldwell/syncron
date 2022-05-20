// Copyright © 2022 David Caldwell <david@porkrind.org>

use std::ffi::OsString;

// Is all this worth a more readable serialization?
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(untagged)]
pub enum MaybeUTF8 {
    UTF8(String),
    Raw(OsString),
}

impl MaybeUTF8 {
    pub fn new(s: OsString) -> MaybeUTF8 {
        match s.to_str() {
            Some(s) => MaybeUTF8::UTF8(String::from(s)),
            None    => MaybeUTF8::Raw(s),
        }
    }
}

impl From<MaybeUTF8> for OsString {
    fn from(s: MaybeUTF8) -> Self {
        match s {
            MaybeUTF8::Raw(s)  => s,
            MaybeUTF8::UTF8(s) => OsString::from(s),
        }
    }
}
