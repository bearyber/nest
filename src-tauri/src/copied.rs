//! A copied computer (Migration Assistant, a restored backup, copied settings): two installs
//! that share one device id would keep overwriting each other's list. Nest notices and gives
//! one of them its own id, once, without asking (it is invisible plumbing; the one note that
//! follows only asks for a name).
//!
//! How it tells: every launch makes a random token, held in memory and written in this
//! computer's own list. Our own earlier tokens are remembered in settings (`past_tokens`), so
//! a list we wrote ourselves is never "foreign". A list written by a token that isn't ours,
//! seen as two different writes (compared as written, never by clock), means another install is
//! writing it. Our earlier tokens count as ours only on the file as it was when this launch
//! first looked. A list with no token (Nest 0.7.3) proves nothing. The one whose token is larger
//! moves; the other keeps its id and writes its list again at once. After a move the evidence
//! starts over, so it can never loop.

use std::path::Path;
use std::sync::OnceLock;

use crate::devices::{self, DeviceFile};
use crate::settings::Settings;

/// How many of this computer's own earlier tokens are remembered.
pub const MAX_PAST_TOKENS: usize = 10;
/// How many different foreign writes are kept as evidence (two are enough).
const MAX_SEEN: usize = 4;

/// This launch's token: random, made once, never saved except in this computer's own list.
pub fn launch_token() -> &'static str {
    static TOKEN: OnceLock<String> = OnceLock::new();
    TOKEN.get_or_init(|| uuid::Uuid::new_v4().simple().to_string())
}

/// What has been seen of another install writing our list (kept in memory only).
/// Only "which write is this" is compared (token + `updatedAt` as plain text), never a time
/// against this computer's clock: the two installs may have different clocks.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Watch {
    /// Whether this launch has looked at the list yet.
    looked: bool,
    /// The list as first seen this launch (token, updatedAt), if there was one. A file carrying
    /// one of our own earlier tokens is ours only while it is still exactly this one.
    baseline: Option<(String, String)>,
    /// The different foreign writes seen (token, updatedAt).
    seen: Vec<(String, String)>,
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

/// Judge our own list as it is in the folder (`file`). A write is foreign when its token is
/// not this launch's and is not one of our earlier tokens on a file that was already there
/// when this launch first looked (an earlier token on a file that changed since is someone
/// else writing: a copy made while the original was running holds the original's token).
/// Two different foreign writes mean another install is writing this list.
pub fn judge(
    watch: &mut Watch,
    file: Option<&DeviceFile>,
    my_token: &str,
    past: &[String],
) -> Verdict {
    let sig = file.map(|f| (f.token.clone(), f.updated_at.clone()));
    if !watch.looked {
        watch.looked = true;
        watch.baseline = sig.clone();
    }
    let (Some(file), Some(sig)) = (file, sig) else {
        return Verdict::Alone;
    };
    if file.token.is_empty() || file.token == my_token {
        return Verdict::Alone; // ours, or an older Nest: no evidence
    }
    if past.contains(&file.token) && watch.baseline.as_ref() == Some(&sig) {
        return Verdict::Alone; // one of our own earlier writes, from before this launch
    }
    // The same write read again tells nothing new (a stale copy read twice, a slow sync app).
    if watch.seen.contains(&sig) {
        return Verdict::Alone;
    }
    watch.seen.push(sig);
    let extra = watch.seen.len().saturating_sub(MAX_SEEN);
    watch.seen.drain(..extra);
    watch.other_token = file.token.clone();
    if watch.seen.len() < 2 {
        return Verdict::Rewrite;
    }
    if my_token > watch.other_token.as_str() {
        Verdict::Move
    } else {
        // The other install moves. Start the evidence over: if it never does, we see it again.
        watch.seen.clear();
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
    let v = judge(watch, file.as_ref(), my_token, past);
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
        // The file as first seen this launch carries an earlier token of ours.
        let past = vec!["old".to_string()];
        assert_eq!(
            judge(&mut w, Some(&file("old", T1)), "me", &past),
            Verdict::Alone
        );
        assert_eq!(
            judge(&mut w, Some(&file("old", T1)), "me", &past),
            Verdict::Alone
        );
        assert_eq!(judge(&mut w, None, "me", &past), Verdict::Alone);
        assert_eq!(
            judge(&mut w, Some(&file("me", T2)), "me", &past),
            Verdict::Alone
        );
        // 0.7.3: no token at all.
        assert_eq!(
            judge(&mut w, Some(&file("", T3)), "me", &past),
            Verdict::Alone
        );
        assert!(w.seen.is_empty());
    }

    #[test]
    fn one_foreign_write_rewrites_two_move_or_stay() {
        let mut w = Watch::default();
        assert_eq!(
            judge(&mut w, Some(&file("zzz", T1)), "aaa", &[]),
            Verdict::Rewrite
        );
        // The same write read again: nothing new.
        assert_eq!(
            judge(&mut w, Some(&file("zzz", T1)), "aaa", &[]),
            Verdict::Alone
        );
        // A second, different write: "aaa" is the smaller token, so it stays.
        assert_eq!(
            judge(&mut w, Some(&file("zzz", T2)), "aaa", &[]),
            Verdict::Stay
        );
        assert!(w.seen.is_empty());

        let mut w = Watch::default();
        assert_eq!(
            judge(&mut w, Some(&file("aaa", T1)), "zzz", &[]),
            Verdict::Rewrite
        );
        assert_eq!(
            judge(&mut w, Some(&file("aaa", T2)), "zzz", &[]),
            Verdict::Move
        );
    }

    #[test]
    fn a_stale_foreign_file_read_again_is_no_second_write() {
        let mut w = Watch::default();
        for _ in 0..5 {
            assert_ne!(
                judge(&mut w, Some(&file("zzz", T1)), "aaa", &[]),
                Verdict::Move
            );
        }
        assert_eq!(w.seen.len(), 1);
    }

    #[test]
    fn times_are_never_compared_only_which_write_it_is() {
        // Unreadable or "earlier" times (another computer's clock) change nothing: two different
        // writes are evidence, one write is not.
        let mut w = Watch::default();
        assert_eq!(
            judge(&mut w, Some(&file("zzz", "yesterday")), "aaa", &[]),
            Verdict::Rewrite
        );
        assert_eq!(
            judge(&mut w, Some(&file("zzz", "yesterday")), "aaa", &[]),
            Verdict::Alone
        );
        assert_eq!(
            judge(
                &mut w,
                Some(&file("zzz", "2001-01-01T00:00:00Z")),
                "aaa",
                &[]
            ),
            Verdict::Stay
        );
    }

    #[test]
    fn the_copy_holds_the_originals_running_token() {
        // Settings copied while the original ran: the copy's past tokens hold the original's
        // current token. Written by the original after the copy launched, it is foreign.
        let mut w = Watch::default();
        let past = vec!["orig".to_string()];
        assert_eq!(
            judge(&mut w, Some(&file("orig", T1)), "xcopy", &past),
            Verdict::Alone // as first seen: before this launch
        );
        assert_eq!(
            judge(&mut w, Some(&file("orig", T2)), "xcopy", &past),
            Verdict::Rewrite
        );
        assert_eq!(
            judge(&mut w, Some(&file("orig", T3)), "xcopy", &past),
            Verdict::Move // "xcopy" > "orig"
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
