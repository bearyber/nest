//! A copied computer (Migration Assistant, a restored backup, copied settings): two installs
//! that share one device id would keep overwriting each other's list. Nest notices and gives
//! one of them its own id, once, without asking (it is invisible plumbing; the one note that
//! follows only asks for a name).
//!
//! How it tells: every launch makes a random token, held in memory and written in this
//! computer's own list. Our own earlier tokens are remembered in settings (`past_tokens`), so
//! a list we wrote ourselves is never "foreign". A list written by a token that isn't ours,
//! newer than our own last write, and seen at two different times, means another install is
//! writing it. A list with no token (Nest 0.7.3) proves nothing. The one whose token is larger
//! moves; the other keeps its id and writes its list again at once. After a move the evidence
//! starts over, so it can never loop.

use std::path::Path;
use std::sync::OnceLock;

use crate::devices::{self, DeviceFile};
use crate::settings::Settings;

/// How many of this computer's own earlier tokens are remembered.
pub const MAX_PAST_TOKENS: usize = 10;
/// How many different write times are kept as evidence (two are enough).
const MAX_SEEN: usize = 4;

/// This launch's token: random, made once, never saved except in this computer's own list.
pub fn launch_token() -> &'static str {
    static TOKEN: OnceLock<String> = OnceLock::new();
    TOKEN.get_or_init(|| uuid::Uuid::new_v4().simple().to_string())
}

/// What has been seen of another install writing our list (kept in memory only).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Watch {
    /// The different `updatedAt` times at which a foreign token was seen.
    seen: Vec<String>,
    /// The token of the newest foreign write.
    other_token: String,
}

/// What to do after looking at our own list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Nothing foreign (or nothing that proves anything).
    Alone,
    /// Another install wrote our list once: write ours again (no change of id yet).
    Rewrite,
    /// Seen twice, and this install's token is the larger: take a new id.
    Move,
    /// Seen twice, and the other install's token is the larger: keep the id (it moves).
    Stay,
}

/// Remember `token` as one of this computer's own (newest last, at most `MAX_PAST_TOKENS`).
/// Returns whether the list changed.
pub fn remember_token(past: &mut Vec<String>, token: &str) -> bool {
    if past.iter().any(|t| t == token) {
        return false;
    }
    past.push(token.to_string());
    let extra = past.len().saturating_sub(MAX_PAST_TOKENS);
    past.drain(..extra);
    true
}

fn parse(t: &str) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    chrono::DateTime::parse_from_rfc3339(t).ok()
}

/// Judge our own list as it is in the folder (`file`). `our_last_write` is the `updatedAt` of
/// the last list this install wrote (none yet = any foreign write counts).
pub fn judge(
    watch: &mut Watch,
    file: Option<&DeviceFile>,
    my_token: &str,
    past: &[String],
    our_last_write: Option<&str>,
) -> Verdict {
    let Some(file) = file else {
        return Verdict::Alone;
    };
    if file.token.is_empty() || file.token == my_token || past.contains(&file.token) {
        return Verdict::Alone; // ours, or an older Nest: no evidence
    }
    let Some(written) = parse(&file.updated_at) else {
        return Verdict::Alone;
    };
    // Older than our own last write: a stale copy coming back from a sync app, not a write
    // made after ours.
    if our_last_write
        .and_then(parse)
        .is_some_and(|ours| written <= ours)
    {
        return Verdict::Alone;
    }
    // The same write read again tells nothing new.
    if watch.seen.iter().any(|s| s == &file.updated_at) {
        return Verdict::Alone;
    }
    watch.seen.push(file.updated_at.clone());
    let extra = watch.seen.len().saturating_sub(MAX_SEEN);
    watch.seen.drain(..extra);
    watch.other_token = file.token.clone();
    if watch.seen.len() < 2 {
        return Verdict::Rewrite;
    }
    if my_token > watch.other_token.as_str() {
        Verdict::Move
    } else {
        // The other install moves. Start the count over: if it never does, we see it again.
        *watch = Watch::default();
        Verdict::Stay
    }
}

/// One look at our own list in `dir`. On `Rewrite` and `Stay`, `last` is cleared so the next
/// write really happens (it would otherwise be skipped as "nothing changed").
pub fn step(
    dir: &Path,
    own_id: &str,
    my_token: &str,
    past: &[String],
    watch: &mut Watch,
    last: &mut Option<DeviceFile>,
) -> Verdict {
    let file = devices::read_own(dir, own_id);
    let ours = last.as_ref().map(|l| l.updated_at.clone());
    let v = judge(watch, file.as_ref(), my_token, past, ours.as_deref());
    if matches!(v, Verdict::Rewrite | Verdict::Stay) {
        *last = None;
    }
    v
}

/// The settings change for a move: a new id, and nothing of the original's request
/// bookkeeping (else its old "mark done" requests would be sent again and its dealt-with notes
/// would belong to a computer that is no longer this one). The old list file is never touched.
/// Sets the flag for the one note that asks for a name.
pub fn move_to_new_id(s: &mut Settings, new_id: String) {
    s.device_id = Some(new_id);
    s.outgoing_requests.clear();
    s.handled_requests.clear();
    s.handled_seq.clear();
    s.waiting_since.clear();
    s.request_seq = 0;
    s.copied_note = true;
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "11111111-1111-4111-8111-111111111111";

    fn file(token: &str, at: &str) -> DeviceFile {
        DeviceFile {
            device_id: ID.into(),
            token: token.into(),
            updated_at: at.into(),
            ..Default::default()
        }
    }

    const T1: &str = "2026-10-22T10:00:01+08:00";
    const T2: &str = "2026-10-22T10:00:02+08:00";
    const T3: &str = "2026-10-22T10:00:03+08:00";

    #[test]
    fn our_own_and_older_files_are_never_evidence() {
        let mut w = Watch::default();
        let past = vec!["old".to_string()];
        assert_eq!(judge(&mut w, None, "me", &past, None), Verdict::Alone);
        assert_eq!(
            judge(&mut w, Some(&file("me", T1)), "me", &past, None),
            Verdict::Alone
        );
        assert_eq!(
            judge(&mut w, Some(&file("old", T1)), "me", &past, None),
            Verdict::Alone
        );
        // 0.7.3: no token at all.
        assert_eq!(
            judge(&mut w, Some(&file("", T1)), "me", &past, None),
            Verdict::Alone
        );
        assert_eq!(w, Watch::default());
    }

    #[test]
    fn one_foreign_write_rewrites_two_move_or_stay() {
        let mut w = Watch::default();
        assert_eq!(
            judge(&mut w, Some(&file("zzz", T1)), "aaa", &[], None),
            Verdict::Rewrite
        );
        // The same write read again: nothing new.
        assert_eq!(
            judge(&mut w, Some(&file("zzz", T1)), "aaa", &[], None),
            Verdict::Alone
        );
        // A second, different write: "aaa" is the smaller token, so it stays.
        assert_eq!(
            judge(&mut w, Some(&file("zzz", T2)), "aaa", &[], Some(T1)),
            Verdict::Stay
        );
        assert_eq!(w, Watch::default());

        let mut w = Watch::default();
        assert_eq!(
            judge(&mut w, Some(&file("aaa", T1)), "zzz", &[], None),
            Verdict::Rewrite
        );
        assert_eq!(
            judge(&mut w, Some(&file("aaa", T2)), "zzz", &[], None),
            Verdict::Move
        );
    }

    #[test]
    fn a_stale_copy_older_than_our_last_write_is_no_evidence() {
        let mut w = Watch::default();
        // We wrote at T3; a sync app brings back foreign files from T1 and T2.
        assert_eq!(
            judge(&mut w, Some(&file("zzz", T1)), "aaa", &[], Some(T3)),
            Verdict::Alone
        );
        assert_eq!(
            judge(&mut w, Some(&file("zzz", T2)), "aaa", &[], Some(T3)),
            Verdict::Alone
        );
        assert_eq!(
            judge(&mut w, Some(&file("zzz", T3)), "aaa", &[], Some(T3)),
            Verdict::Alone
        );
        assert_eq!(w, Watch::default());
    }

    #[test]
    fn a_time_that_cannot_be_read_is_no_evidence() {
        let mut w = Watch::default();
        assert_eq!(
            judge(&mut w, Some(&file("zzz", "yesterday")), "aaa", &[], None),
            Verdict::Alone
        );
    }

    #[test]
    fn past_tokens_are_kept_to_the_cap_newest_last() {
        let mut past = vec![];
        for i in 0..15 {
            assert!(remember_token(&mut past, &format!("t{i}")));
        }
        assert!(!remember_token(&mut past, "t14"));
        assert_eq!(past.len(), MAX_PAST_TOKENS);
        assert_eq!(past.last().map(String::as_str), Some("t14"));
        assert!(!past.contains(&"t0".to_string()));
    }

    #[test]
    fn a_move_forgets_the_originals_requests_and_notes() {
        use crate::devices::{Handled, StatusRequest};
        let mut s = Settings {
            device_id: Some(ID.into()),
            request_seq: 77,
            ..Default::default()
        };
        s.outgoing_requests.push(StatusRequest {
            id: "r".into(),
            seq: 77,
            ..Default::default()
        });
        s.handled_requests.push(Handled {
            id: "h".into(),
            from_device: "x".into(),
            seq: 3,
            applied: true,
            reason: None,
        });
        s.handled_seq.insert("x".into(), 3);
        s.waiting_since.insert(
            "w".into(),
            crate::devices::WaitingSince {
                since: "2026-10-01T00:00:00Z".into(),
                from_device: "x".into(),
                seq: 4,
            },
        );
        s.local_marker = "W".into();
        s.device_name = Some("PC".into());
        move_to_new_id(&mut s, "22222222-2222-4222-8222-222222222222".into());
        assert_eq!(
            s.device_id.as_deref(),
            Some("22222222-2222-4222-8222-222222222222")
        );
        assert!(s.outgoing_requests.is_empty() && s.handled_requests.is_empty());
        assert!(s.handled_seq.is_empty() && s.waiting_since.is_empty());
        assert_eq!(s.request_seq, 0);
        assert!(s.copied_note);
        // Never a change of letter or name by itself.
        assert_eq!(
            (s.local_marker.as_str(), s.device_name.as_deref()),
            ("W", Some("PC"))
        );
    }
}
