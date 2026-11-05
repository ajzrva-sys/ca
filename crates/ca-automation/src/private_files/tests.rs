use super::*;
use std::fs;
use std::os::unix::{
    ffi::OsStringExt,
    fs::{symlink, MetadataExt, PermissionsExt},
};

struct Fixture {
    _temp: tempfile::TempDir,
    files: PrivateFiles,
    source: PathBuf,
    destination: PathBuf,
    from: String,
    to: String,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("private-source");
        let destination = temp.path().join("private-destination");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&destination).unwrap();
        let files = PrivateFiles::open(&temp.path().join("journal")).unwrap();
        let from = files.bind_folder(&source).unwrap().reference;
        let to = files.bind_folder(&destination).unwrap().reference;
        Self {
            _temp: temp,
            files,
            source,
            destination,
            from,
            to,
        }
    }
    fn select(&self) -> Selection {
        self.files
            .select(&self.from, Some(&self.to), FileCriteriaV1::default())
            .unwrap()
    }
    fn run(&self, selection: &Selection) -> FileMoveProgressV1 {
        self.files
            .execute(
                &self
                    .files
                    .bind_move(&selection.reference, &self.to)
                    .unwrap(),
                &MoveControl::default(),
            )
            .unwrap()
    }
}

#[test]
fn private_batch_moves_1200_nested_native_names_and_keeps_frozen_identity() {
    let f = Fixture::new();
    for i in 0..1200 {
        let path = f
            .source
            .join(format!("group-{}/nested/entry-{i}.pdf", i % 12));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, (i as u64).to_le_bytes()).unwrap();
    }
    for name in ["unicode-日本語.pdf", "space ' \" $(touch nope).pdf"] {
        fs::write(f.source.join(name), b"private bytes").unwrap();
    }
    let native = std::ffi::OsString::from_vec(b"native-\xff.pdf".to_vec());
    let native_created = match fs::write(f.source.join(&native), b"native") {
        Ok(()) => true,
        Err(e) => {
            assert_eq!(
                e.raw_os_error(),
                Some(libc::EILSEQ),
                "unexpected native-name filesystem failure"
            );
            false
        }
    };
    assert_eq!(
        NativePath::new(Path::new(&native)).path(),
        Path::new(&native)
    );
    symlink("../outside-missing", f.source.join("link.pdf")).unwrap();
    let selection = f.select();
    let total = 1203 + usize::from(native_created);
    assert_eq!(selection.members.len(), total);
    let p = f.run(&selection);
    assert_eq!(p.moved, total);
    assert_eq!(p.errors, 0);
    assert!(f.source.join("group-1/nested").is_dir());
    if native_created {
        assert_eq!(fs::read(f.destination.join(&native)).unwrap(), b"native");
    }
    assert_eq!(
        fs::read_link(f.destination.join("link.pdf")).unwrap(),
        Path::new("../outside-missing")
    );
    for i in 0..1200 {
        assert_eq!(
            fs::read(
                f.destination
                    .join(format!("group-{}/nested/entry-{i}.pdf", i % 12))
            )
            .unwrap(),
            (i as u64).to_le_bytes()
        );
    }
    assert_eq!(
        f.run(&selection),
        p,
        "identical retry returns the same aggregate"
    );
    for member in &selection.members {
        assert!(f
            .files
            .resource(&member.reference)
            .unwrap()
            .path
            .path()
            .starts_with(&f.destination));
    }
    // Replacements at old locations never enter an already completed selection.
    fs::write(f.source.join("unicode-日本語.pdf"), b"replacement").unwrap();
    assert_eq!(f.run(&selection).moved, total);
    assert_eq!(
        fs::read(f.source.join("unicode-日本語.pdf")).unwrap(),
        b"replacement"
    );
}

#[test]
fn private_selection_rejects_forged_replaced_and_changed_resources() {
    let f = Fixture::new();
    fs::write(f.source.join("a.pdf"), b"initial").unwrap();
    assert!(f.files.selection("files_../../escape").is_err());
    assert!(f
        .files
        .resource("file_00000000000000000000000000000000")
        .is_err());
    let s = f.select();
    fs::write(f.source.join("a.pdf"), b"changed").unwrap();
    let p = f.run(&s);
    assert_eq!(p.errors, 1);
    assert!(!f.destination.join("a.pdf").exists());
    let moved = f.source.with_extension("old");
    fs::rename(&f.source, &moved).unwrap();
    fs::create_dir(&f.source).unwrap();
    assert!(f.files.folder(&f.from).is_err());
}

#[test]
fn private_batch_skips_collisions_and_excludes_nested_destination() {
    let f = Fixture::new();
    fs::write(f.source.join("a.pdf"), b"source").unwrap();
    fs::write(f.destination.join("a.pdf"), b"keep").unwrap();
    assert_eq!(f.run(&f.select()).skipped, 1);
    assert_eq!(fs::read(f.destination.join("a.pdf")).unwrap(), b"keep");
    let nested = f.source.join("nested-destination");
    fs::create_dir(&nested).unwrap();
    fs::write(nested.join("keep.pdf"), b"keep").unwrap();
    let dest = f.files.bind_folder(&nested).unwrap().reference;
    let s = f
        .files
        .select(&f.from, Some(&dest), FileCriteriaV1::default())
        .unwrap();
    assert_eq!(s.members.len(), 1);
    assert!(f.files.bind_move(&s.reference, &dest).is_ok());
    let s = f
        .files
        .select(&f.from, None, FileCriteriaV1::default())
        .unwrap();
    assert!(f.files.bind_move(&s.reference, &dest).is_err());
}

#[test]
fn private_batch_never_follows_replaced_parent_symlink() {
    let f = Fixture::new();
    fs::create_dir(f.source.join("sub")).unwrap();
    fs::write(f.source.join("sub/a"), b"a").unwrap();
    let s = f.select();
    fs::rename(f.source.join("sub"), f.source.join("old")).unwrap();
    symlink(f.source.join("old"), f.source.join("sub")).unwrap();
    let binding = f.files.bind_move(&s.reference, &f.to).unwrap();
    assert!(f.files.execute(&binding, &MoveControl::default()).is_err());
    assert!(f.source.join("old/a").exists());
    assert!(!f.destination.join("sub/a").exists());
}

#[test]
fn private_batch_reconciles_same_filesystem_commit_and_receipt_faults() {
    for point in [
        "before_intent",
        "after_intent",
        "after_parking_intent",
        "after_park",
        "after_park_receipt",
        "after_publish",
        "before_entry_receipt",
        "after_receipt",
        "before_batch_receipt",
        "after_batch_receipt",
    ] {
        let f = Fixture::new();
        fs::write(f.source.join("a"), b"original").unwrap();
        let s = f.select();
        let b = f.files.bind_move(&s.reference, &f.to).unwrap();
        let fault = |p: &str, _: usize| {
            if p == point {
                anyhow::bail!("injected {p}");
            }
            Ok(())
        };
        assert!(
            f.files
                .execute(
                    &b,
                    &MoveControl {
                        fault: Some(&fault),
                        ..Default::default()
                    }
                )
                .is_err(),
            "{point}"
        );
        let reopened = PrivateFiles::open(f.files.root()).unwrap();
        let p = reopened.execute(&b, &MoveControl::default()).unwrap();
        assert_eq!(p.moved, 1, "{point}");
        assert_eq!(fs::read(f.destination.join("a")).unwrap(), b"original");
        assert!(!f.source.join("a").exists());
    }
}

#[test]
fn private_batch_cancellation_parks_and_continues_without_replay() {
    let f = Fixture::new();
    for i in 0..4 {
        fs::write(f.source.join(format!("{i}")), [i]).unwrap();
    }
    let s = f.select();
    let b = f.files.bind_move(&s.reference, &f.to).unwrap();
    let count = std::cell::Cell::new(0);
    let report = |p: &FileMoveProgressV1| count.set(p.moved);
    let cancel = || count.get() == 2;
    let p = f
        .files
        .execute(
            &b,
            &MoveControl {
                progress: Some(&report),
                cancel: Some(&cancel),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(p.moved, 2);
    assert_eq!(p.remaining, 2);
    assert!(p.cancelled);
    let p = f.files.execute(&b, &MoveControl::default()).unwrap();
    assert_eq!(p.moved, 4);
    assert_eq!(p.remaining, 0);
}

#[test]
fn private_batch_source_leaf_replacements_are_never_removed_or_moved() {
    for point in ["before_park", "after_park"] {
        let f = Fixture::new();
        let path = f.source.join("a");
        fs::write(&path, b"original").unwrap();
        let s = f.select();
        let b = f.files.bind_move(&s.reference, &f.to).unwrap();
        let changed = std::cell::Cell::new(false);
        let inject = |p: &str, _: usize| {
            if p == point && !changed.replace(true) {
                if path.exists() {
                    fs::rename(&path, f.source.join("retained-original")).unwrap();
                }
                fs::write(&path, b"replacement").unwrap();
            }
            Ok(())
        };
        let p = f
            .files
            .execute(
                &b,
                &MoveControl {
                    fault: Some(&inject),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"replacement");
        if point == "before_park" {
            assert_eq!(p.errors, 1);
            assert!(!f.destination.join("a").exists());
            assert_eq!(
                fs::read(f.source.join("retained-original")).unwrap(),
                b"original"
            );
        } else {
            assert_eq!(p.moved, 1);
            assert_eq!(fs::read(f.destination.join("a")).unwrap(), b"original");
        }
    }
}

#[test]
fn private_batch_existing_hard_link_is_a_collision() {
    let f = Fixture::new();
    fs::write(f.source.join("a"), b"original").unwrap();
    fs::hard_link(f.source.join("a"), f.destination.join("a")).unwrap();
    let p = f.run(&f.select());
    assert_eq!(p.skipped, 1);
    assert!(f.source.join("a").exists());
}

#[test]
fn private_native_engine_cancel_resume_cancel_updates_the_receipt() {
    use crate::native::{AutomationEngine, FileMoveHooks};
    use crate::runtime::{ExecutionContext, ExecutionState, EXECUTION_SCHEMA};
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    let f = Fixture::new();
    for i in 0..4 {
        fs::write(f.source.join(format!("{i}")), [i]).unwrap();
    }
    let s = f.select();
    let binding = f.files.bind_move(&s.reference, &f.to).unwrap();
    let context = ExecutionContext {
        schema: EXECUTION_SCHEMA.into(),
        run_id: binding.operation.clone(),
        profile: "fixture".into(),
        state_root: f.files.root().join("engine"),
        lease_root: f.files.root().join("leases"),
        definition_digest: "a".repeat(64),
        resource_binding_digest: "b".repeat(64),
        policy_digest: "c".repeat(64),
        resources: vec!["private-files-fixture".into()],
        runtime_seconds: 60,
        output_bytes: 65536,
    };
    for stop in [1, 3, 4] {
        let cancelled = Arc::new(AtomicBool::new(false));
        let flag = cancelled.clone();
        let progress = move |p: &FileMoveProgressV1| {
            if p.moved >= stop {
                flag.store(true, Ordering::Release);
            }
        };
        let r = AutomationEngine::execute_private_move(
            &context,
            f.files.root().to_path_buf(),
            binding.clone(),
            FileMoveHooks {
                progress: Some(Arc::new(progress)),
                cancelled: Some(cancelled),
            },
        )
        .unwrap();
        let value = r.result.unwrap();
        assert_eq!(value["moved"], stop);
        assert_eq!(value["remaining"], 4 - stop);
        assert_eq!(
            r.state,
            if stop < 4 {
                ExecutionState::NeedsAttention
            } else {
                ExecutionState::Succeeded
            }
        );
    }
}

/// A real device boundary is mandatory. Build output and disposable TMPDIR
/// reside on separate APFS volumes on the qualification host. Other hosts may
/// supply fixture roots; unavailable cross-filesystem coverage fails clearly.
#[test]
fn private_batch_actual_cross_filesystem_metadata_and_faults() {
    let left = std::env::var_os("CA_TEST_MOVE_LEFT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/private-move-fixtures");
            fs::create_dir_all(&path).expect("disposable build fixture root");
            path.canonicalize().expect("canonical build fixture root")
        });
    let right = std::env::var_os("CA_TEST_MOVE_RIGHT")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    assert_ne!(
        fs::metadata(&left).unwrap().dev(),
        fs::metadata(&right).unwrap().dev(),
        "cross-filesystem qualification requires distinct devices; supply CA_TEST_MOVE_LEFT/RIGHT"
    );
    for point in [
        "after_intent",
        "after_parking_intent",
        "after_park",
        "after_park_receipt",
        "after_temp_create",
        "after_temp_intent",
        "after_verify",
        "after_publish",
        "after_publish_receipt",
        "after_source_remove",
        "before_entry_receipt",
        "before_batch_receipt",
    ] {
        let src = tempfile::tempdir_in(&left).unwrap();
        let dest = tempfile::tempdir_in(&right).unwrap();
        let journal = tempfile::tempdir_in(&left).unwrap();
        let files = PrivateFiles::open(&journal.path().join("private")).unwrap();
        let from = files.bind_folder(src.path()).unwrap().reference;
        let to = files.bind_folder(dest.path()).unwrap().reference;
        let source = src.path().join("large.bin");
        fs::write(&source, vec![0xab; 8 * 1024 * 1024]).unwrap();
        fs::set_permissions(&source, fs::Permissions::from_mode(0o640)).unwrap();
        symlink("large.bin", src.path().join("link")).unwrap();
        let meta = fs::symlink_metadata(&source).unwrap();
        let selection = files
            .select(&from, Some(&to), FileCriteriaV1::default())
            .unwrap();
        let binding = files.bind_move(&selection.reference, &to).unwrap();
        let fired = std::cell::Cell::new(false);
        let fault = |p: &str, _: usize| {
            if p == point && !fired.replace(true) {
                anyhow::bail!("injected {p}");
            }
            Ok(())
        };
        assert!(
            files
                .execute(
                    &binding,
                    &MoveControl {
                        fault: Some(&fault),
                        ..Default::default()
                    }
                )
                .is_err(),
            "{point}"
        );
        let p = files.execute(&binding, &MoveControl::default()).unwrap();
        assert_eq!(p.moved, 2, "{point}");
        let actual = fs::symlink_metadata(dest.path().join("large.bin")).unwrap();
        assert_eq!(actual.mode() & 0o7777, meta.mode() & 0o7777);
        assert_eq!((actual.uid(), actual.gid()), (meta.uid(), meta.gid()));
        assert_eq!(
            (actual.mtime(), actual.mtime_nsec()),
            (meta.mtime(), meta.mtime_nsec())
        );
        assert_eq!(
            fs::read(dest.path().join("large.bin")).unwrap(),
            vec![0xab; 8 * 1024 * 1024]
        );
        assert_eq!(
            fs::read_link(dest.path().join("link")).unwrap(),
            Path::new("large.bin")
        );
        assert!(!source.exists());
    }
}

#[test]
fn private_native_process_crash_fixture() {
    let Some(input) = std::env::var_os("CA_TEST_PRIVATE_CRASH_INPUT") else {
        return;
    };
    let (context, root, binding): (crate::runtime::ExecutionContext, PathBuf, MoveBinding) =
        serde_json::from_slice(&fs::read(input).unwrap()).unwrap();
    let progress = |p: &FileMoveProgressV1| {
        if p.moved == 1 {
            std::process::exit(73);
        }
    };
    crate::native::AutomationEngine::execute_private_move(
        &context,
        root,
        binding,
        crate::native::FileMoveHooks {
            progress: Some(std::sync::Arc::new(progress)),
            cancelled: None,
        },
    )
    .unwrap();
    panic!("crash fixture failed to reach a committed entry");
}

#[test]
fn private_native_process_crash_releases_ownership_and_resumes_once() {
    let f = Fixture::new();
    for i in 0..3 {
        fs::write(f.source.join(format!("{i}.pdf")), [i]).unwrap();
    }
    let s = f.select();
    let binding = f.files.bind_move(&s.reference, &f.to).unwrap();
    let context = crate::runtime::ExecutionContext {
        schema: crate::runtime::EXECUTION_SCHEMA.into(),
        run_id: binding.operation.clone(),
        profile: "crash-fixture".into(),
        state_root: f.files.root().join("engine"),
        lease_root: f.files.root().join("leases"),
        definition_digest: "a".repeat(64),
        resource_binding_digest: "b".repeat(64),
        policy_digest: "c".repeat(64),
        resources: vec!["private-files-crash-fixture".into()],
        runtime_seconds: 60,
        output_bytes: 65536,
    };
    let input = f._temp.path().join("crash-input.json");
    fs::write(
        &input,
        serde_json::to_vec(&(&context, f.files.root(), &binding)).unwrap(),
    )
    .unwrap();
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "private_files::tests::private_native_process_crash_fixture",
            "--nocapture",
        ])
        .env("CA_TEST_PRIVATE_CRASH_INPUT", input)
        .output()
        .unwrap();
    assert_eq!(
        child.status.code(),
        Some(73),
        "{}",
        String::from_utf8_lossy(&child.stderr)
    );
    let moved = fs::read_dir(&f.destination)
        .unwrap()
        .filter_map(Result::ok)
        .find(|e| e.file_type().unwrap().is_file())
        .unwrap()
        .path();
    let inode = fs::metadata(&moved).unwrap().ino();
    let receipt = crate::native::AutomationEngine::execute_private_move(
        &context,
        f.files.root().to_path_buf(),
        binding.clone(),
        Default::default(),
    )
    .unwrap();
    assert_eq!(receipt.state, crate::runtime::ExecutionState::Succeeded);
    assert_eq!(receipt.result.as_ref().unwrap()["moved"], 3);
    assert_eq!(receipt.actions.len(), 1);
    assert_eq!(
        fs::metadata(&moved).unwrap().ino(),
        inode,
        "completed entry must not be replayed"
    );
    let again = crate::native::AutomationEngine::execute_private_move(
        &context,
        f.files.root().to_path_buf(),
        binding,
        Default::default(),
    )
    .unwrap();
    assert_eq!(again.result, receipt.result);
    for i in 0..3 {
        assert_eq!(
            fs::read(f.destination.join(format!("{i}.pdf"))).unwrap(),
            [i]
        );
        assert!(!f.source.join(format!("{i}.pdf")).exists());
    }
}
