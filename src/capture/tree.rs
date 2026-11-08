//! The session tree (MAX-202): the session browser's default view, the sessions grouped by
//! device, then by month, week and day as `group_by` says. A group is replayed
//! as one, so a device group holds only sessions whose bytes share one setup; the text format
//! of their `.log` isn't part of it, as a replay reads the `.raw` alone. Nothing moves
//! on disk: the tree is built from the listed sessions each time
use chrono::{Datelike, Duration, NaiveDate};
use serde::{Deserialize, Serialize};

use super::session::{CaptureDevice, Session, hostname, time_span};

/// A time level of the tree, coarse to fine
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GroupLevel {
    Month,
    Week,
    Day,
}

/// The time levels by default: months, ISO weeks, days
pub const DEFAULT_GROUP_BY: [GroupLevel; 3] =
    [GroupLevel::Month, GroupLevel::Week, GroupLevel::Day];

/// What a node of the tree is
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    Device,
    Month,
    Week,
    Day,
    Session,
}

/// A node of the session tree: a group, or one session. `key` names it across a reload (the
/// expanded groups stay open), `sessions` are the indexes of all the sessions under it in the
/// list it was built from, the oldest first as a group replays them, `size` their files'
#[derive(Debug, Clone, PartialEq)]
pub struct SessionNode {
    pub kind: NodeKind,
    pub key: String,
    pub label: String,
    pub sessions: Vec<usize>,
    pub size: u64,
    pub children: Vec<SessionNode>,
}

impl SessionNode {
    fn leaf(sessions: &[Session], index: usize, parent: &str) -> Self {
        let session = &sessions[index];
        Self {
            kind: NodeKind::Session,
            key: format!("{parent}/{}", session.stem),
            label: session.time_span(),
            sessions: vec![index],
            size: session.size,
            children: Vec::new(),
        }
    }

    /// From the start of its oldest session to the end of its newest
    pub fn time_span(&self, sessions: &[Session]) -> String {
        let session = |index: Option<&usize>| index.map(|&index| &sessions[index]);
        time_span(
            session(self.sessions.first()).and_then(|oldest| oldest.started),
            session(self.sessions.last()).and_then(|newest| newest.ended),
        )
    }
}

/// `sessions` as a flat list of leaves, in their order: the session browser's list view
pub fn session_list(sessions: &[Session]) -> Vec<SessionNode> {
    (0..sessions.len())
        .map(|index| SessionNode::leaf(sessions, index, ""))
        .collect()
}

/// The tree of `sessions` (MAX-202). At the top, one group per device, the one
/// with the newest session first; a session without a meta or a start can't be grouped and
/// stays alone there. Under each, the time levels of `group_by` (in any order, coarse to
/// fine once sorted), the newest first. A week is an ISO week, Monday to Sunday; under months
/// it belongs to the month of its Thursday, so it never splits. A time group with a single
/// child is that child, so a week of one day shows the day and a day of one session the
/// session
pub fn session_tree(sessions: &[Session], group_by: &[GroupLevel]) -> Vec<SessionNode> {
    let mut levels = group_by.to_vec();
    levels.sort();
    levels.dedup();

    let mut devices: Vec<(&CaptureDevice, Vec<usize>)> = Vec::new();
    let mut alone = Vec::new();
    for (index, session) in sessions.iter().enumerate() {
        let (Some(meta), Some(_)) = (&session.meta, session.started) else {
            alone.push(index);
            continue;
        };
        match devices
            .iter_mut()
            .find(|(device, _)| **device == meta.device)
        {
            Some((_, indexes)) => indexes.push(index),
            None => devices.push((&meta.device, vec![index])),
        }
    }

    let mut roots: Vec<SessionNode> = devices
        .into_iter()
        .map(|(device, indexes)| {
            let key = format!("{device:?}");
            let children = group(sessions, &indexes, &levels, &key);
            node(
                sessions,
                NodeKind::Device,
                key,
                device_label(device),
                children,
            )
        })
        .collect();
    roots.extend(
        alone
            .into_iter()
            .map(|index| SessionNode::leaf(sessions, index, "")),
    );
    roots.sort_by(|a, b| newest_stem(sessions, b).cmp(newest_stem(sessions, a)));
    roots
}

/// The children of a group holding `indexes`, by the first of `levels`
fn group(
    sessions: &[Session],
    indexes: &[usize],
    levels: &[GroupLevel],
    key: &str,
) -> Vec<SessionNode> {
    let Some((&level, finer)) = levels.split_first() else {
        let mut leaves: Vec<_> = (indexes.iter())
            .map(|&index| SessionNode::leaf(sessions, index, key))
            .collect();
        leaves.sort_by(|a, b| newest_stem(sessions, b).cmp(newest_stem(sessions, a)));
        return leaves;
    };
    let weeks = levels.contains(&GroupLevel::Week);
    let mut buckets: Vec<(NaiveDate, Vec<usize>)> = Vec::new();
    for &index in indexes {
        let Some(day) = sessions[index].started.map(|started| started.date_naive()) else {
            continue;
        };
        let start = period_start(level, day, weeks);
        match buckets.iter_mut().find(|(at, _)| *at == start) {
            Some((_, bucket)) => bucket.push(index),
            None => buckets.push((start, vec![index])),
        }
    }
    buckets.sort_by_key(|bucket| std::cmp::Reverse(bucket.0));
    buckets
        .into_iter()
        .map(|(start, bucket)| {
            let key = format!("{key}/{}", period_key(level, start));
            let children = group(sessions, &bucket, finer, &key);
            match <[SessionNode; 1]>::try_from(children) {
                Ok([only]) => only,
                Err(children) => node(
                    sessions,
                    level.kind(),
                    key,
                    period_label(level, start),
                    children,
                ),
            }
        })
        .collect()
}

fn node(
    sessions: &[Session],
    kind: NodeKind,
    key: String,
    label: String,
    children: Vec<SessionNode>,
) -> SessionNode {
    let mut indexes: Vec<usize> = children
        .iter()
        .flat_map(|child| child.sessions.clone())
        .collect();
    indexes.sort_by(|&a, &b| sessions[a].stem.cmp(&sessions[b].stem));
    SessionNode {
        kind,
        key,
        label,
        size: indexes.iter().map(|&index| sessions[index].size).sum(),
        sessions: indexes,
        children,
    }
}

/// The newest session's stem under `node`: the stems start with the date
fn newest_stem<'a>(sessions: &'a [Session], node: &SessionNode) -> &'a str {
    (node.sessions.iter())
        .map(|&index| sessions[index].stem.as_str())
        .max()
        .unwrap_or_default()
}

impl GroupLevel {
    fn kind(self) -> NodeKind {
        match self {
            Self::Month => NodeKind::Month,
            Self::Week => NodeKind::Week,
            Self::Day => NodeKind::Day,
        }
    }
}

/// The first day of the period of `level` holding `day`. With `weeks`, a month starts from
/// the Thursday of `day`'s week, so the week stays whole in one month
fn period_start(level: GroupLevel, day: NaiveDate, weeks: bool) -> NaiveDate {
    let monday = day - Duration::days(day.weekday().num_days_from_monday().into());
    match level {
        GroupLevel::Month => {
            let day = if weeks {
                monday + Duration::days(3)
            } else {
                day
            };
            day.with_day(1).unwrap_or(day)
        }
        GroupLevel::Week => monday,
        GroupLevel::Day => day,
    }
}

/// The part of a node's key a period gives: the same for the same period after a reload
fn period_key(level: GroupLevel, start: NaiveDate) -> String {
    match level {
        GroupLevel::Month => start.format("%Y-%m").to_string(),
        GroupLevel::Week => start.format("%G-W%V").to_string(),
        GroupLevel::Day => start.format("%Y-%m-%d").to_string(),
    }
}

/// How a period reads, whole on its own since a collapsed level can hide its parent:
/// `2026-11`, `2026-W45 (Nov 02–08)`, `Sat 2026-11-07`
pub fn period_label(level: GroupLevel, start: NaiveDate) -> String {
    match level {
        GroupLevel::Month => start.format("%Y-%m").to_string(),
        GroupLevel::Week => {
            let end = start + Duration::days(6);
            let end = if end.month() == start.month() {
                end.format("%d")
            } else {
                end.format("%b %d")
            };
            format!(
                "{} ({}–{end})",
                start.format("%G-W%V"),
                start.format("%b %d")
            )
        }
        GroupLevel::Day => start.format("%a %Y-%m-%d").to_string(),
    }
}

/// How a device group reads: `ttyUSB0 115200 8N1 UTF8`, the line settings left out when it
/// has none (a TCP link), the machine added when it isn't this one
fn device_label(device: &CaptureDevice) -> String {
    let mut label = device.address.tag().into_owned();
    if let Some(line) = device.line {
        label.push_str(&format!(" {} {}", line.baud, line.framing));
    }
    label.push_str(&format!(" {}", device.encoding));
    if device.host != hostname() {
        label.push_str(&format!(" @{}", device.host));
    }
    label
}

#[cfg(test)]
#[path = "../tests/capture/tree.rs"]
mod tests;
