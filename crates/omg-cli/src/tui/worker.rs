//! Data worker: the only owner of the SQLite connection used by the interface. The UI thread sends
//! requests and renders whatever has arrived, so a slow query can never freeze a keypress.
use omg_core::{
    Index, Status,
    catalog::{Finding, Ignore},
    cleanup::{DeletePlan, DeleteReport, Marked},
    planning::{History, Plan},
    platform,
    rollup::{Child, Totals},
};
use std::{
    path::PathBuf,
    sync::mpsc::{Receiver, Sender, channel},
};

pub enum Request {
    Summary,
    Dir(Vec<u8>),
    Preview(String),
    RecordPreview(Box<Plan>),
    Ignore(Box<Finding>),
    RemoveIgnore(i64),
    Mark {
        path: Vec<u8>,
        allocated: u64,
    },
    Unmark(Vec<i64>),
    PrepareDelete(Vec<i64>),
    /// Add to OMG Trash and immediately prepare a one-item delete plan.
    DeleteNow {
        path: Vec<u8>,
        allocated: u64,
    },
    Delete(Box<DeletePlan>),
}

pub struct Summary {
    pub status: Status,
    pub rollups: bool,
    pub findings: Vec<Finding>,
    pub top: Vec<Child>,
    pub history: Vec<History>,
    pub ignores: Vec<Ignore>,
    pub marked: Vec<Marked>,
    pub trend_delta: Option<i64>,
    pub root: Option<PathBuf>,
    pub home: Option<PathBuf>,
}

pub enum Response {
    Summary(Box<Summary>),
    Dir {
        path: Vec<u8>,
        children: Vec<Child>,
        total: Option<Totals>,
    },
    Plan(Box<Plan>),
    DeletePlan {
        plan: Box<DeletePlan>,
        added_now: bool,
    },
    Deleted(Box<DeleteReport>),
    Info(String),
    Error(String),
}

fn summary(index: &Index) -> anyhow::Result<Summary> {
    let status = index.status()?;
    let rollups = index.rollups_available()?;
    Ok(Summary {
        root: status.root.as_deref().and_then(|r| platform::path(r).ok()),
        home: platform::home().ok(),
        findings: index.findings(1000, 0, false)?,
        top: if rollups {
            index.children(b"", 8, 0)?
        } else {
            vec![]
        },
        history: index.history()?,
        ignores: index.ignores()?,
        marked: index.marked()?,
        trend_delta: index
            .trend(30)
            .ok()
            .and_then(|t| t.indexed_allocated_change),
        rollups,
        status,
    })
}

pub fn spawn(state: PathBuf) -> (Sender<Request>, Receiver<Response>) {
    let (request_tx, request_rx) = channel::<Request>();
    let (response_tx, response_rx) = channel::<Response>();
    std::thread::spawn(move || {
        let mut index = match Index::open(&state) {
            Ok(index) => index,
            Err(error) => {
                let _ = response_tx.send(Response::Error(format!("{error:#}")));
                return;
            }
        };
        let home = platform::home().unwrap_or_default();
        while let Ok(first) = request_rx.recv() {
            // Coalesce bursts: many queued refreshes cost one query.
            let mut batch = vec![first];
            batch.extend(request_rx.try_iter());
            let mut refresh = false;
            for request in batch {
                let send = |r: Response| response_tx.send(r).is_ok();
                let outcome: anyhow::Result<()> = (|| {
                    match request {
                        Request::Summary => refresh = true,
                        Request::Dir(path) => {
                            let children = index.children(&path, 500, 0)?;
                            let total = index.subtree(&path)?;
                            send(Response::Dir {
                                path,
                                children,
                                total,
                            });
                        }
                        Request::Preview(id) => {
                            send(Response::Plan(Box::new(
                                index.preview(std::slice::from_ref(&id))?,
                            )));
                        }
                        Request::RecordPreview(plan) => {
                            index.record_preview(&plan)?;
                            send(Response::Info(
                                "Preview recorded. Nothing was changed.".into(),
                            ));
                            refresh = true;
                        }
                        Request::Ignore(finding) => {
                            index.ignore_finding(&finding, false)?;
                            send(Response::Info(
                                "Ignored. Undo it in the Ignores tab.".into(),
                            ));
                            refresh = true;
                        }
                        Request::RemoveIgnore(id) => {
                            index.remove_ignore(id)?;
                            refresh = true;
                        }
                        Request::Mark { path, allocated } => {
                            let gate = index.mark(&path, allocated, &home)?;
                            let note = match gate {
                                omg_core::cleanup::Gate::Warn(_) => {
                                    " (inside a Git repository — review before deleting)"
                                }
                                _ => "",
                            };
                            send(Response::Info(format!(
                                "Added to OMG Trash{note}. Nothing changed on disk."
                            )));
                            refresh = true;
                        }
                        Request::Unmark(ids) => {
                            for id in ids {
                                index.unmark(id)?;
                            }
                            send(Response::Info("Removed from OMG Trash.".into()));
                            refresh = true;
                        }
                        Request::PrepareDelete(ids) => {
                            send(Response::DeletePlan {
                                plan: Box::new(index.prepare_delete(&ids, &home)?),
                                added_now: false,
                            });
                        }
                        Request::DeleteNow { path, allocated } => {
                            let existed = index.marked()?.iter().any(|m| m.path == path);
                            index.mark(&path, allocated, &home)?;
                            let id = index
                                .marked()?
                                .into_iter()
                                .find(|m| m.path == path)
                                .map(|m| m.id)
                                .ok_or_else(|| anyhow::anyhow!("could not stage the item"))?;
                            send(Response::DeletePlan {
                                plan: Box::new(index.prepare_delete(&[id], &home)?),
                                added_now: !existed,
                            });
                            refresh = true;
                        }
                        Request::Delete(plan) => {
                            let fingerprint = plan.fingerprint.clone();
                            send(Response::Deleted(Box::new(index.delete_permanently(
                                &plan,
                                &fingerprint,
                                &home,
                            )?)));
                            refresh = true;
                        }
                    }
                    Ok(())
                })();
                if let Err(error) = outcome {
                    let _ = response_tx.send(Response::Error(format!("{error:#}")));
                }
            }
            if refresh {
                let message = match summary(&index) {
                    Ok(s) => Response::Summary(Box::new(s)),
                    Err(error) => Response::Error(format!("{error:#}")),
                };
                if response_tx.send(message).is_err() {
                    return;
                }
            }
        }
    });
    (request_tx, response_rx)
}
