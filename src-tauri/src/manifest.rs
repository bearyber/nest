//! `.project.json` and other JSON files: atomic writes (CLAUDE.md #6), hidden manifests,
//! and reads that keep unknown fields (CLAUDE.md #5).

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

use serde_json::Value as Json;

/// Write JSON atomically: unique dot-tmp file (`create_new`) → fsync → rename over `path`.
/// `hidden` also sets the Windows hidden attribute on the tmp file, so the final file is
/// never visible un-hidden. The tmp file is removed if anything fails.
pub fn write_json_atomic(path: &Path, value: &Json, hidden: bool) -> io::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no parent folder"))?;
    let tmp = dir.join(format!(".nest-{}.tmp", uuid::Uuid::new_v4().simple()));
    let mut bytes = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;
    bytes.push(b'\n');

    let mut file = open_new(&tmp, hidden)?;
    let result = (|| {
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, path)?;
        sync_dir(dir)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// Read a manifest (or any JSON object). Unknown fields are kept as-is.
/// A UTF-8 BOM (some Windows editors add one) is ignored.
pub fn read_json_object(path: &Path) -> Result<Json, String> {
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let value: Json = serde_json::from_str(text).map_err(|e| e.to_string())?;
    if value.is_object() {
        Ok(value)
    } else {
        Err("not a JSON object".into())
    }
}

/// Change only `status` (and `doneAt`: set when marking done, removed when marking active) in
/// the manifest at `file`, atomically, keeping every other field, the key order and the hidden
/// flag. Refuses if the manifest now belongs to another project, or if something else changed
/// the file while Nest was working on it.
pub fn update_status(file: &Path, expected_id: &str, status: &str) -> Result<(), String> {
    update_status_with(file, expected_id, status, || {})
}

/// `update_status` with a hook that runs just before the final check (tests use it to
/// simulate a sync app editing the file mid-way).
pub fn update_status_with(
    file: &Path,
    expected_id: &str,
    status: &str,
    before_check: impl FnOnce(),
) -> Result<(), String> {
    let stamp = |p: &Path| fs::metadata(p).map(|m| (m.modified().ok(), m.len()));
    let before = stamp(file).map_err(|e| format!("Couldn't read the project file: {e}"))?;
    let mut manifest = read_json_object(file)
        .map_err(|e| format!("The project file can't be read ({e}), so it was left untouched."))?;
    if manifest["id"].as_str() != Some(expected_id) {
        return Err(format!(
            "This folder now holds a different project. Refresh the list ({}) and try again.",
            crate::error::refresh_keys()
        ));
    }
    manifest["status"] = Json::String(status.to_string());
    // Archive (M5): "done since" drives the "ready to archive" suggestion.
    if status == "done" {
        if !manifest["doneAt"].is_string() {
            manifest["doneAt"] = Json::String(chrono::Local::now().to_rfc3339());
        }
    } else if let Some(obj) = manifest.as_object_mut() {
        obj.remove("doneAt");
    }
    before_check();
    if stamp(file).ok() != Some(before) {
        return Err(
            "The project file was changed by something else just now (a sync app?). Nothing was overwritten; try again."
                .into(),
        );
    }
    write_json_atomic(file, &manifest, true)
        .map_err(|e| format!("Couldn't save the project file: {e}"))
}

#[cfg(windows)]
fn open_new(path: &Path, hidden: bool) -> io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
    const FILE_ATTRIBUTE_NORMAL: u32 = 0x80;
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .attributes(if hidden {
            FILE_ATTRIBUTE_HIDDEN
        } else {
            FILE_ATTRIBUTE_NORMAL
        })
        .open(path)
}

/// macOS hides dot-files already; the name is the hidden flag.
#[cfg(not(windows))]
fn open_new(path: &Path, _hidden: bool) -> io::Result<File> {
    OpenOptions::new().write(true).create_new(true).open(path)
}

/// Persist the rename itself. Not possible through std on Windows.
#[cfg(unix)]
pub(crate) fn sync_dir(dir: &Path) -> io::Result<()> {
    File::open(dir)?.sync_all()
}

#[cfg(not(unix))]
pub(crate) fn sync_dir(_dir: &Path) -> io::Result<()> {
    Ok(())
}

/// Whether the Windows hidden attribute is set (always true-ish on macOS via the dot name).
#[cfg(windows)]
pub fn is_hidden(path: &Path) -> io::Result<bool> {
    use std::os::windows::fs::MetadataExt;
    Ok(fs::metadata(path)?.file_attributes() & 0x2 != 0)
}

#[cfg(not(windows))]
pub fn is_hidden(path: &Path) -> io::Result<bool> {
    fs::metadata(path)?;
    Ok(path
        .file_name()
        .is_some_and(|n| n.to_string_lossy().starts_with('.')))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn writes_hidden_and_leaves_no_tmp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".project.json");
        write_json_atomic(&path, &json!({ "schema": 1 }), true).unwrap();
        assert!(is_hidden(&path).unwrap());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1, "no tmp left");
    }

    #[test]
    fn renaming_over_an_existing_hidden_manifest_works() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".project.json");
        write_json_atomic(&path, &json!({ "status": "active" }), true).unwrap();
        write_json_atomic(&path, &json!({ "status": "done" }), true).unwrap();
        assert_eq!(read_json_object(&path).unwrap()["status"], "done");
        assert!(is_hidden(&path).unwrap());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn unknown_fields_and_order_survive_a_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".project.json");
        let original = json!({
            "schema": 1, "id": "x", "jobCode": "VX-L01",
            "links": { "ops app": { "projectId": "p1" }, "frameio": { "a": 1 } },
            "futureThing": [1, 2, 3]
        });
        write_json_atomic(&path, &original, true).unwrap();
        let mut read = read_json_object(&path).unwrap();
        read["status"] = json!("done");
        write_json_atomic(&path, &read, true).unwrap();
        let again = read_json_object(&path).unwrap();
        assert_eq!(again["links"]["frameio"]["a"], 1);
        assert_eq!(again["futureThing"], json!([1, 2, 3]));
        let keys: Vec<&String> = again.as_object().unwrap().keys().collect();
        assert_eq!(
            keys,
            ["schema", "id", "jobCode", "links", "futureThing", "status"]
        );
    }

    #[test]
    fn failed_write_cleans_up_tmp() {
        let dir = tempfile::tempdir().unwrap();
        // Renaming onto an existing non-empty folder fails on both OSes.
        let target = dir.path().join("occupied");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("keep.txt"), "user file").unwrap();
        assert!(write_json_atomic(&target, &json!({}), false).is_err());
        let names: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["occupied"], "tmp file removed");
        assert_eq!(
            fs::read_to_string(target.join("keep.txt")).unwrap(),
            "user file"
        );
    }

    #[test]
    fn update_status_changes_only_status() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".project.json");
        let original = json!({
            "schema": 1, "id": "abc", "jobCode": "VX-L01", "status": "active",
            "links": { "frameio": { "x": 1 } }, "future": true
        });
        write_json_atomic(&path, &original, true).unwrap();
        update_status(&path, "abc", "done").unwrap();
        let mut after = read_json_object(&path).unwrap();
        // Marking done also stamps doneAt (a parseable date), and nothing else.
        let done_at = after["doneAt"].as_str().unwrap().to_string();
        assert!(chrono::DateTime::parse_from_rfc3339(&done_at).is_ok());
        after.as_object_mut().unwrap().remove("doneAt");
        let mut expected = original.clone();
        expected["status"] = json!("done");
        assert_eq!(after, expected);
        let keys: Vec<&String> = after.as_object().unwrap().keys().collect();
        assert_eq!(
            keys,
            ["schema", "id", "jobCode", "status", "links", "future"]
        );
        assert!(is_hidden(&path).unwrap());
        // Marking done again keeps the original date; marking active removes it.
        update_status(&path, "abc", "done").unwrap();
        assert_eq!(read_json_object(&path).unwrap()["doneAt"], json!(done_at));
        update_status(&path, "abc", "active").unwrap();
        assert!(read_json_object(&path).unwrap().get("doneAt").is_none());
    }

    #[test]
    fn update_status_refuses_other_project_or_concurrent_edit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".project.json");
        write_json_atomic(&path, &json!({ "id": "abc", "status": "active" }), true).unwrap();

        let err = update_status(&path, "other-id", "done").unwrap_err();
        assert!(err.contains("different project"), "{err}");

        // A sync app rewrites the file while Nest is mid-change: theirs must survive.
        let err = update_status_with(&path, "abc", "done", || {
            write_json_atomic(
                &path,
                &json!({ "id": "abc", "status": "active", "note": "theirs, longer" }),
                true,
            )
            .unwrap();
        })
        .unwrap_err();
        assert!(err.contains("changed by something else"), "{err}");
        assert_eq!(read_json_object(&path).unwrap()["note"], "theirs, longer");
    }

    #[test]
    fn manifest_with_bom_is_readable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".project.json");
        fs::write(&path, "\u{feff}{ \"id\": \"abc\" }").unwrap();
        assert_eq!(read_json_object(&path).unwrap()["id"], "abc");
    }

    #[test]
    fn unreadable_manifest_is_an_error_not_a_panic() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".project.json");
        fs::write(&path, "{ not json").unwrap();
        assert!(read_json_object(&path).is_err());
        fs::write(&path, "[1]").unwrap();
        assert!(read_json_object(&path).is_err());
    }
}
