//! Folder sizes (M5 P4). Temp dirs only.

use std::fs;

use nest_lib::sizes::measure;

#[test]
fn sizes_per_subfolder_and_type_match_what_was_written() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path();
    fs::create_dir_all(p.join("01_FOOTAGE/A001")).unwrap();
    fs::create_dir_all(p.join("02_AUDIO")).unwrap();
    fs::create_dir_all(p.join("03_EMPTY")).unwrap();
    fs::write(p.join("01_FOOTAGE/A001/clip.MOV"), vec![0u8; 1000]).unwrap(); // case-insensitive
    fs::write(p.join("01_FOOTAGE/A001/clip.braw"), vec![0u8; 500]).unwrap();
    fs::write(p.join("01_FOOTAGE/still.png"), vec![0u8; 40]).unwrap();
    fs::write(p.join("02_AUDIO/mix.wav"), vec![0u8; 300]).unwrap();
    fs::write(p.join("02_AUDIO/notes.txt"), vec![0u8; 7]).unwrap();
    fs::write(p.join("NOTES.md"), vec![0u8; 9]).unwrap(); // loose file in the project folder
    fs::write(p.join(".project.json"), b"{}").unwrap(); // 2 bytes, counted too

    let s = measure(p, 1_700_000_000_000);
    assert_eq!(s.measured_at, 1_700_000_000_000);
    assert_eq!(s.total, 1000 + 500 + 40 + 300 + 7 + 9 + 2);
    assert_eq!(s.files, 7);
    assert_eq!(s.unreadable, 0);
    let names: Vec<&str> = s.folders.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(
        names,
        ["01_FOOTAGE", "02_AUDIO", "", "03_EMPTY"],
        "biggest first, loose files, empty last"
    );
    let footage = &s.folders[0];
    assert_eq!(
        (
            footage.total,
            footage.video,
            footage.images,
            footage.audio,
            footage.other
        ),
        (1540, 1500, 40, 0, 0)
    );
    let audio = &s.folders[1];
    assert_eq!((audio.total, audio.audio, audio.other), (307, 300, 7));
    let loose = &s.folders[2];
    assert_eq!((loose.total, loose.other), (11, 11));
    assert_eq!(s.folders[3].total, 0);
}

#[cfg(unix)]
#[test]
fn links_are_never_followed() {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path();
    fs::create_dir(p.join("real")).unwrap();
    fs::write(p.join("real/a.mov"), vec![0u8; 100]).unwrap();
    std::os::unix::fs::symlink(p.join("real"), p.join("link")).unwrap();
    std::os::unix::fs::symlink(p.join("real/a.mov"), p.join("real/b.mov")).unwrap();
    let s = measure(p, 0);
    assert_eq!(
        s.total, 100,
        "neither the linked folder nor the linked file is counted"
    );
    assert_eq!(s.files, 1);
}

#[test]
fn a_missing_folder_is_unreadable_not_an_error() {
    let tmp = tempfile::tempdir().unwrap();
    let s = measure(&tmp.path().join("gone"), 0);
    assert_eq!((s.total, s.unreadable), (0, 1));
}
