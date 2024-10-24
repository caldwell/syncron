// Copyright © 2024 David Caldwell <david@porkrind.org>

use std::io::{Error, ErrorKind};

use rocket::futures::stream::Stream;
use rocket::http::hyper::body::Bytes;
use rocket::response::stream::stream;
use tokio::io::AsyncBufReadExt; // lines()

use rocket::futures::TryStreamExt;
use tokio_stream::wrappers::LinesStream;
use tokio_util::io::StreamReader; // map_err()

#[derive(Clone, Debug)]
pub struct ServerSentEvent {
    pub event: String,
    pub data: String,
}

impl Default for ServerSentEvent {
    fn default() -> Self {
        ServerSentEvent { event: "message".to_string(), data: String::new() }
    }
}

pub fn server_sent_events_stream(bytes_stream: impl Stream<Item = reqwest::Result<Bytes>>) -> impl Stream<Item = ServerSentEvent> {
    let line_stream = LinesStream::new(
        StreamReader::new(
            bytes_stream
                .map_err(|e| Error::new(ErrorKind::Other, format!("{e}"))),
        )
            .lines(),
    );
    stream! {
        #[allow(unused_variables)] let mut last_id = None;
        #[allow(unused_variables)] let mut reconnect_time_ms = None;
        let mut event = ServerSentEvent::default();
        for await line in line_stream {
            #[allow(unused_assignments)]
            match line {
                Err(e) => { warn!("Got error on stream: {e}"); break },
                Ok(line) => {
                    debug!("Line: {line}");
                    // https://html.spec.whatwg.org/multipage/server-sent-events.html#event-stream-interpretation
                    if line.is_empty() {
                        if !event.data.is_empty() { yield event }
                        event = ServerSentEvent::default();
                    } else if line.starts_with(':') { // ignore comment lines--they're just keep-alives
                    } else {
                        let (field, value) = line.split_once(':').unwrap_or((line.as_str(), ""));
                        match field {
                            "event" => event.event = value.to_owned(),
                            "data" => {
                                // spec says to unilaterally add a \n to the end of the received data and then
                                // remove the final \n when dispatching. This is equivalent.
                                if event.data != "" {
                                    event.data.push_str("\n")
                                };
                                event.data.push_str(value)
                            },
                            "id" => last_id = Some(value.to_owned()),
                            "retry" => { if let Ok(retry) = u64::from_str_radix(value, 10) { reconnect_time_ms = Some(retry) } },
                            _ => { /* spec says ignore anything else */ }
                        }
                    }
                },
            }
        }
    }
}
