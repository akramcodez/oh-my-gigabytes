#![cfg(target_os = "linux")]
use omg_core::{Index, ScanOptions, planning, platform, policy, scanner};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64},
    },
};
#[test]
fn explicit_preview_is_read_only_and_execution_cannot_bypass_denial() {
    let state = tempfile::tempdir().unwrap();
    fs::set_permissions(state.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("large.zip");
    fs::File::create(&file)
        .unwrap()
        .set_len(101 * 1024 * 1024)
        .unwrap();
    let mut index = Index::open(state.path()).unwrap();
    scanner::scan(
        &mut index,
        &ScanOptions {
            root: root.path().into(),
            workers: 1,
            exclusions: vec![],
            full: true,
            publish_partial: false,
        },
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicU64::new(0)),
    )
    .unwrap();
    let finding = index.findings(1000, 0, false).unwrap().pop().unwrap();
    let metadata = fs::metadata(&file).unwrap();
    assert!(index.preview(&[]).is_err());
    let plan = index
        .preview(&[finding.id.clone(), finding.id.clone()])
        .unwrap();
    assert_eq!(plan.items.len(), 1);
    assert_eq!(plan.actionable, 0);
    assert!(plan.dry_run);
    index.record_preview(&plan).unwrap();
    assert_eq!(index.history().unwrap().len(), 1);
    assert!(index.execute(&plan).is_err());
    assert_eq!(metadata.len(), fs::metadata(&file).unwrap().len());
    fs::rename(&file, root.path().join("old")).unwrap();
    fs::File::create(&file)
        .unwrap()
        .set_len(101 * 1024 * 1024)
        .unwrap();
    assert!(planning::revalidate(root.path(), &finding.entry).is_err());
    let stale = index.preview(&[finding.id]).unwrap();
    assert!(stale.items[0].decision.contains("Stale"));
    assert!(policy::cleanup_denial(root.path(), &finding.entry).contains("disabled"));
    assert!(platform::path(&plan.items[0].path).unwrap().is_relative());
}
