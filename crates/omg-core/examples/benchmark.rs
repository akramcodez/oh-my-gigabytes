use omg_core::{Index, ScanOptions, scanner};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64},
    },
    time::Instant,
};
fn main() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let state = tempfile::tempdir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(state.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    for directory in 0..100 {
        let path = root.path().join(format!("d{directory}"));
        std::fs::create_dir(&path)?;
        for file in 0..100 {
            std::fs::write(path.join(format!("f{file}")), [0u8; 64])?;
        }
    }
    let mut index = Index::open(state.path())?;
    let options = ScanOptions {
        root: root.path().into(),
        workers: 2,
        exclusions: vec![],
        full: true,
    };
    for label in ["cold-index (OS cache warm)", "warm-reconciliation"] {
        let report = scanner::scan(
            &mut index,
            &options,
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicU64::new(0)),
        )?;
        println!("{label}: {}", serde_json::to_string(&report)?);
    }
    let started = Instant::now();
    drop(index);
    let index = Index::open(state.path())?;
    index.status()?;
    index.entries(50, 0)?;
    println!("warm startup query ms: {}", started.elapsed().as_millis());
    Ok(())
}
