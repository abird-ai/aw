//! Private fixture roots shared by the library and binary unit tests.
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

pub(crate) fn temp_dir(prefix: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    reserve_dir(&std::env::temp_dir(), prefix, stamp, &NEXT)
}

fn reserve_dir(base: &Path, prefix: &str, stamp: u128, next: &AtomicUsize) -> PathBuf {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(false);
    for _ in 0..32 {
        let nonce = next.fetch_add(1, Ordering::Relaxed);
        let path = base.join(format!(
            "aw-test-{prefix}-{}-{stamp}-{nonce}",
            std::process::id()
        ));
        match builder.create(&path) {
            Ok(()) => return path,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => panic!("cannot reserve fixture directory {path:?}: {error}"),
        }
    }
    panic!("fixture directory candidates already exist under {base:?}");
}

#[test]
fn concurrent_fixture_roots_skip_preclaimed_directory() {
    use std::{collections::HashSet, sync::Arc};

    let base = temp_dir("reservation-regression");
    let existing = base.join(format!("aw-test-fixture-{}-0-0", std::process::id()));
    std::fs::create_dir(&existing).unwrap();
    std::fs::write(existing.join("sentinel"), b"preserve existing bytes").unwrap();
    let next = Arc::new(AtomicUsize::new(0));
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let base = base.clone();
            let next = next.clone();
            std::thread::spawn(move || reserve_dir(&base, "fixture", 0, &next))
        })
        .collect();
    let roots: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    let distinct: HashSet<_> = roots.iter().collect();
    assert_eq!(distinct.len(), 8);
    assert!(!distinct.contains(&existing));
    assert_eq!(
        std::fs::read(existing.join("sentinel")).unwrap(),
        b"preserve existing bytes"
    );
    std::fs::remove_dir_all(base).unwrap();
}
