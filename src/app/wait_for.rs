//! `wait-for` (MAX-219): a job waits for one of its texts to come in, the script's expect.
//! Each watch is the app's, by its number, which the job's `Wait::Text` holds: several jobs
//! (a client's, a trigger's) can wait at once, each for its own texts
use std::time::Instant;

use super::{App, triggers::StreamMatcher};
use crate::commands::{
    jobs::Wait,
    report::{Answer, Note, Report},
    value::{Value, escaped},
};

/// The texts one `wait-for` waits for, until when, each found across reads
#[derive(Debug, Clone)]
pub struct TextWatch {
    /// Its number, given when it starts
    pub id: u64,
    pub until: Instant,
    texts: Vec<Vec<u8>>,
    matchers: Vec<StreamMatcher>,
}

impl TextWatch {
    /// `None` when a text is empty, which would match anything
    pub fn new(texts: Vec<Vec<u8>>, until: Instant) -> Option<Self> {
        let matchers = texts.iter().map(|text| StreamMatcher::new(text));
        Some(Self {
            id: 0,
            until,
            matchers: matchers.collect::<Option<_>>()?,
            texts,
        })
    }

    /// The text that came in with these bytes: the one ending first, the first given on a tie
    pub fn feed(&mut self, chunk: &[u8]) -> Option<&[u8]> {
        let ends = self.matchers.iter_mut().map(|matcher| matcher.feed(chunk));
        let (index, _) = (ends.enumerate())
            .filter_map(|(index, end)| Some((index, end?)))
            .min_by_key(|(_, end)| *end)?;
        Some(&self.texts[index])
    }

    /// Its texts as `send` reads them, quoted: `'OK' or 'ERROR'`
    pub fn texts_shown(&self) -> String {
        let quoted: Vec<_> = (self.texts.iter())
            .map(|text| format!("'{}'", escaped(text)))
            .collect();
        quoted.join(" or ")
    }
}

impl App {
    /// The watches fed what came in: a job whose text came goes on, its note saying which;
    /// a client (or a trigger, a profile) gets the text as the answer
    pub fn watch_texts(&mut self, chunk: &[u8]) {
        let mut came = Vec::new();
        self.text_watches.retain_mut(|watch| {
            let id = watch.id;
            match watch.feed(chunk) {
                Some(text) => {
                    came.push((id, text.to_vec()));
                    false
                }
                None => true,
            }
        });
        for (id, text) in came {
            self.end_text_watch(id, Ok(text));
        }
    }

    /// Every watch failed, with `why`: the link closed, a replay took its place
    pub fn fail_text_watches(&mut self, why: &str) {
        for watch in std::mem::take(&mut self.text_watches) {
            let failed = format!("wait-for: {why} before {} came in", watch.texts_shown());
            self.end_text_watch(watch.id, Err(failed));
        }
    }

    /// The watches kept in step with their jobs: one whose job is gone (cancelled, failed)
    /// dropped, one past its time failing its job
    pub fn sync_text_watches(&mut self, now: Instant) {
        let jobs = &self.jobs;
        (self.text_watches)
            .retain(|watch| jobs.waiting(|wait| waits_for(wait, watch.id)).is_some());
        let (late, kept) = std::mem::take(&mut self.text_watches)
            .into_iter()
            .partition(|watch| now >= watch.until);
        self.text_watches = kept;
        for watch in late {
            let failed = format!("wait-for: {} didn't come in", watch.texts_shown());
            self.end_text_watch(watch.id, Err(failed));
        }
    }

    /// The job waiting for watch `id` goes on with the text that came, or fails; none waiting
    /// (the user's own input has no job), nothing
    fn end_text_watch(&mut self, id: u64, text: Result<Vec<u8>, String>) {
        let Some(caller) = self.jobs.waiting(|wait| waits_for(wait, id)) else {
            return;
        };
        let report = match text {
            Ok(text) => {
                let came = format!("wait-for: '{}' came in", escaped(&text));
                let mut report = Report::from(Note::info(came));
                // the user has it in the note: an answer would open a popup
                if !caller.origin.is_user() {
                    let value = Value::Text(String::from_utf8_lossy(&text).into_owned());
                    report.answer = Some(Answer {
                        query: "wait-for",
                        value,
                    });
                }
                report
            }
            Err(failed) => Report::failed(failed),
        };
        if let Some(job) = caller.job {
            self.jobs.resume(job);
        }
        self.present(&caller, report);
    }
}

fn waits_for(wait: &Wait, id: u64) -> bool {
    matches!(wait, Wait::Text { watch, .. } if *watch == id)
}

#[cfg(test)]
#[path = "../tests/app/wait_for.rs"]
mod tests;
