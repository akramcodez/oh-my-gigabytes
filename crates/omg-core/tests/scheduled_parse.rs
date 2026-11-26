//! Reading back what a registered job runs, so doctor can tell when its omg is gone.
use omg_core::scheduled::{
    Job, Spec, arguments, plist, plist_arguments, systemd_arguments, systemd_units, task_arguments,
};

fn spec(job: Job) -> Spec {
    let base = std::env::temp_dir().join("odd $HOME %h & <x> 'q'");
    Spec {
        job,
        executable: base.join("bin").join("omg"),
        state_dir: base.join("state dir"),
        hour: 4,
    }
}

#[test]
fn plist_and_systemd_arguments_read_back_exactly() {
    for job in [Job::Scan, Job::AutoDelete] {
        let spec = spec(job);
        let expected = arguments(&spec).unwrap();
        assert_eq!(
            plist_arguments(&plist(&spec).unwrap()),
            Some(expected.clone())
        );
        let (service, _) = systemd_units(&spec).unwrap();
        assert_eq!(systemd_arguments(&service), Some(expected));
    }
    assert_eq!(plist_arguments("<plist></plist>"), None);
    assert_eq!(
        systemd_arguments("[Service]\nExecStart=/bin/omg run\n"),
        None
    );
}

#[test]
fn task_xml_arguments_split_like_task_scheduler() {
    let xml = "<Exec>\r\n<Command>C:\\Program Files\\omg\\omg.exe</Command>\r\n<Arguments>--state-dir \"C:\\Users\\a b\\AppData\\Local\\omg\" auto-delete run</Arguments>\r\n</Exec>";
    assert_eq!(
        task_arguments(xml).unwrap(),
        [
            "C:\\Program Files\\omg\\omg.exe",
            "--state-dir",
            "C:\\Users\\a b\\AppData\\Local\\omg",
            "auto-delete",
            "run"
        ]
    );
    let quoted = "<Command>\"C:\\omg &amp; co\\omg.exe\"</Command>";
    assert_eq!(task_arguments(quoted).unwrap(), ["C:\\omg & co\\omg.exe"]);
    assert_eq!(
        task_arguments("ERROR: The system cannot find the file"),
        None
    );
}
