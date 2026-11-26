//! Data worker: the only owner of the SQLite connection used by the interface. The UI thread sends
//! requests and renders whatever has arrived, so a slow query can never freeze a keypress.
use omg_core::{
    Index, Status,
    catalog::Finding,
    cleanup::{DeletePlan, DeleteReport, Marked},
    cleanup::{Gate, Kind},
    planning::Plan,
    platform,
    rollup::{Child, Totals},
};
use std::{
    path::PathBuf,
    sync::mpsc::{Receiver, Sender, channel},
};

pub enum Request {
    Summary,
    Volume(PathBuf),
    Dir(Vec<u8>),
    Preview(String),
    RecordPreview(Box<Plan>),
    Mark {
        path: Vec<u8>,
        allocated: u64,
    },
    Unmark(Vec<i64>),
    /// Remove lines of the deleted log; `None` clears it.
    ForgetDeleted(Option<Vec<i64>>),
    PrepareDelete(Vec<i64>),
    /// Plan the deletion of these cache folders around the protected items inside them. `undo`
    /// are saved items to take back out of OMG Trash if the user cancels.
    PrepareDeleteKeeping {
        ids: Vec<i64>,
        undo: Vec<i64>,
    },
    /// Add to OMG Trash and immediately prepare a one-item delete plan.
    DeleteNow {
        path: Vec<u8>,
        allocated: u64,
    },
    Delete(Box<DeletePlan>),
    /// The backend forgets a plan that is being replaced by another one.
    DiscardPlan(String),
    /// The user cancelled a confirmation: take back what the review added to OMG Trash (`undo`),
    /// forget the plan, then say so. One request, so no later reply overwrites "Nothing was deleted".
    CancelPlan {
        fingerprint: String,
        undo: Vec<i64>,
    },
    SetAutoTrash(bool),
    SetTheme(&'static str),
    /// Progress of a scan run by another process.
    PollRunning,
    /// Ask Docker what is unused and plan its removal (the safe kinds only).
    DockerPlan,
    /// The user accepted the permission screen: turn auto delete on and register its job.
    AutoDeleteGrant {
        days: u32,
        risky: bool,
        plugged_in_only: bool,
    },
    AutoDeleteOff,
    /// Settings that need no new permission.
    AutoDeleteTune {
        days: Option<u32>,
        plugged_in_only: Option<bool>,
        narrow: bool,
    },
    /// Keep an item in OMG Trash out of automatic runs, or let it back in.
    Hold(i64, bool),
    /// Remove a reviewed Docker plan through the Docker API.
    #[cfg_attr(not(feature = "permanent-delete"), allow(dead_code))]
    DockerRemove(Box<omg_core::docker::Plan>),
}

/// What the user can do with a row in Browse.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowMark {
    Deletable,
    /// A cache or build/dependency folder its tool recreates.
    Regenerable,
    Warn(String),
    Locked(String),
}

pub struct Summary {
    pub status: Status,
    pub rollups: bool,
    pub findings: Vec<Finding>,
    pub top: Vec<Child>,
    pub marked: Vec<Marked>,
    pub trend_delta: Option<i64>,
    pub root: Option<PathBuf>,
    pub home: Option<PathBuf>,
    pub auto_trash: bool,
    pub theme: &'static str,
    /// (capacity, used) bytes read from the filesystem now, not from the scan record.
    pub volume: Option<(u64, u64)>,
    pub freed: u64,
    /// Folders that grew between the last two complete scans, largest growth first.
    pub grew: Vec<omg_core::service::trend::Grown>,
    /// What omg deleted in the last 20 days, newest first; automatic runs' lines come first.
    pub deleted: Vec<omg_core::index::DeletedRecord>,
    pub auto: omg_core::cleanup::auto::Preview,
}

pub enum Response {
    Summary(Box<Summary>),
    Volume(Option<(u64, u64)>),
    Dir {
        path: Vec<u8>,
        children: Vec<Child>,
        marks: Vec<RowMark>,
        total: Option<Totals>,
    },
    /// `locked` is whether anything holds the scan lock. A held lock with no scan row is not a
    /// scan: it is housekeeping (pruning old scans) or a scan that has not recorded itself yet.
    Running {
        scan: Option<omg_core::index::RunningScan>,
        locked: bool,
    },
    Plan(Box<Plan>),
    DeletePlan {
        plan: Box<DeletePlan>,
        /// Saved items this review put in OMG Trash; cancelling takes them back out.
        undo: Vec<i64>,
    },
    Deleted(Box<DeleteReport>),
    DockerPlan(Box<omg_core::docker::Plan>, Box<omg_core::docker::Report>),
    #[cfg_attr(not(feature = "permanent-delete"), allow(dead_code))]
    DockerRemoved(Box<omg_core::docker::Outcome>),
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
        marked: index.marked()?,
        trend_delta: index
            .trend(30)
            .ok()
            .and_then(|t| t.indexed_allocated_change),
        auto_trash: index.auto_trash()?,
        theme: index.theme()?,
        freed: index.freed_total()?,
        grew: index.grew(3).map(|g| g.items).unwrap_or_default(),
        deleted: {
            let mut deleted = index.deleted_log()?;
            deleted.sort_by_key(|d| d.source != "auto");
            deleted
        },
        auto: platform::home()
            .ok()
            .map(|home| index.auto_preview(&home))
            .transpose()?
            .unwrap_or_default(),
        volume: status
            .root
            .as_deref()
            .and_then(|r| platform::path(r).ok())
            .and_then(|root| platform::volume_sample(&root)),
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
        if let Ok(Some(text)) = index.auto_take_message() {
            let _ = response_tx.send(Response::Info(text));
        }
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
                        Request::Volume(root) => {
                            send(Response::Volume(platform::volume_sample(&root)));
                        }
                        Request::Dir(path) => {
                            let children = index.children(&path, 500, 0)?;
                            let total = index.subtree(&path)?;
                            let marks = children
                                .iter()
                                .map(|child| match index.entry(&child.path) {
                                    Ok(Some(entry)) => match index.verdict_for(&entry, &home) {
                                        Ok(v) => match (v.gate, v.kind) {
                                            (Gate::Denied(why), _) => RowMark::Locked(why),
                                            (Gate::Warn(why), _) => RowMark::Warn(why),
                                            (Gate::Allowed, Kind::Cache) => RowMark::Regenerable,
                                            (Gate::Allowed, _) => RowMark::Deletable,
                                        },
                                        Err(e) => RowMark::Locked(e.to_string()),
                                    },
                                    _ => RowMark::Locked("not in the index".into()),
                                })
                                .collect();
                            send(Response::Dir {
                                path,
                                children,
                                marks,
                                total,
                            });
                        }
                        Request::PollRunning => {
                            let scan = index.running_scan()?;
                            let locked = omg_core::index::lock_held(&index.state, "scan.lock");
                            send(Response::Running { scan, locked });
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
                        Request::Mark { path, allocated } => {
                            let gate = index.mark(&path, allocated, &home)?;
                            let note = match gate {
                                omg_core::cleanup::Gate::Warn(_) => {
                                    " (inside a Git repository; review before deleting)"
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
                        Request::ForgetDeleted(ids) => {
                            index.forget_deleted(ids.as_deref())?;
                            send(Response::Info(
                                if ids.is_none() {
                                    "Cleared the deleted log. Nothing changed on disk."
                                } else {
                                    "Removed the line from the deleted log."
                                }
                                .into(),
                            ));
                            refresh = true;
                        }
                        Request::PrepareDelete(ids) => {
                            send(Response::DeletePlan {
                                plan: Box::new(index.prepare_delete(&ids, &home)?),
                                undo: vec![],
                            });
                        }
                        Request::PrepareDeleteKeeping { ids, undo } => {
                            send(Response::DeletePlan {
                                plan: Box::new(
                                    index.prepare_delete_keeping_protected(&ids, &home)?,
                                ),
                                undo,
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
                            // Staging is undone when the review cannot proceed, so a failed or
                            // empty review never leaves an item behind in OMG Trash.
                            let plan = match index.prepare_delete(&[id], &home) {
                                Ok(plan) => plan,
                                Err(error) => {
                                    if !existed {
                                        index.unmark(id)?;
                                    }
                                    refresh = true;
                                    return Err(error);
                                }
                            };
                            // A folder that can still be deleted around its protected items stays staged
                            // for that choice; cancelling takes it back out.
                            if plan.items.is_empty() && plan.keepable.is_empty() && !existed {
                                index.unmark(id)?;
                            }
                            send(Response::DeletePlan {
                                plan: Box::new(plan),
                                undo: if existed { vec![] } else { vec![id] },
                            });
                            refresh = true;
                        }
                        Request::SetAutoTrash(on) => {
                            index.set_auto_trash(on)?;
                            send(Response::Info(if on {
                                "Scans will add cache folders to OMG Trash (nothing is deleted without asking).".into()
                            } else {
                                "Scans will no longer add anything to OMG Trash.".into()
                            }));
                            refresh = true;
                        }
                        Request::SetTheme(name) => {
                            index.set_theme(name)?;
                            send(Response::Info(format!("Theme set to {name}.")));
                            refresh = true;
                        }
                        Request::AutoDeleteGrant {
                            days,
                            risky,
                            plugged_in_only,
                        } => {
                            use omg_core::scheduled::{Job, Scheduler, Spec};
                            index.grant_auto_delete(days, risky, plugged_in_only)?;
                            let spec = Spec {
                                job: Job::AutoDelete,
                                executable: std::env::current_exe()?,
                                state_dir: index.state.clone(),
                                hour: 0,
                            };
                            refresh = true;
                            if let Err(error) = Scheduler::for_job(&home, Job::AutoDelete)
                                .and_then(|s| s.enable(&spec))
                            {
                                index.revoke_auto_delete()?;
                                anyhow::bail!(
                                    "could not register the background job, so auto delete stays off: {error:#}"
                                );
                            }
                            send(Response::Info(format!(
                                "Auto delete is on: items in OMG Trash for {} are deleted in the background. Turn it off any time in the menu (m).",
                                super::period_name(days)
                            )));
                        }
                        Request::AutoDeleteOff => {
                            use omg_core::scheduled::{Job, Scheduler};
                            index.revoke_auto_delete()?;
                            refresh = true;
                            let removed = Scheduler::for_job(&home, Job::AutoDelete)
                                .and_then(|s| s.disable());
                            send(Response::Info(match removed {
                                Ok(_) => {
                                    "Auto delete is off. Nothing will be deleted automatically."
                                        .into()
                                }
                                Err(error) => format!(
                                    "Auto delete is off. The background job could not be removed ({error:#}); it does nothing while auto delete is off."
                                ),
                            }));
                        }
                        Request::AutoDeleteTune {
                            days,
                            plugged_in_only,
                            narrow,
                        } => {
                            index.tune_auto_delete(days, plugged_in_only, narrow)?;
                            refresh = true;
                            send(Response::Info("Auto delete settings saved.".into()));
                        }
                        Request::Hold(id, keep) => {
                            index.hold(id, keep)?;
                            refresh = true;
                            send(Response::Info(if keep {
                                "Kept: auto delete will not take this item. It stays in OMG Trash."
                                    .into()
                            } else {
                                "Auto delete may take this item again once it is due.".into()
                            }));
                        }
                        Request::DiscardPlan(fingerprint) => index.discard_plan(&fingerprint),
                        Request::CancelPlan { fingerprint, undo } => {
                            index.discard_plan(&fingerprint);
                            refresh |= !undo.is_empty();
                            for id in undo {
                                index.unmark(id)?;
                            }
                            send(Response::Info("Cancelled. Nothing was deleted.".into()));
                        }
                        Request::DockerPlan => {
                            match omg_core::docker::Engine::find(&home)? {
                                None => send(Response::Info("Docker isn't running, so there is nothing to check. Start Docker and try again.".into())),
                                Some(engine) => {
                                    let report = engine.report()?;
                                    let plan = omg_core::docker::make_plan(&report, &omg_core::docker::Kind::DEFAULT);
                                    send(Response::DockerPlan(Box::new(plan), Box::new(report)))
                                }
                            };
                        }
                        #[cfg(feature = "permanent-delete")]
                        Request::DockerRemove(plan) => {
                            let Some(engine) = omg_core::docker::Engine::find(&home)? else {
                                anyhow::bail!("Docker stopped; nothing was removed");
                            };
                            if engine.endpoint() != plan.endpoint {
                                anyhow::bail!(
                                    "a different Docker is running now; nothing was removed"
                                );
                            }
                            let operation = index.docker_intent(&plan)?;
                            let outcome = engine.remove(&plan, &plan.fingerprint)?;
                            index.docker_outcome(operation, &outcome)?;
                            send(Response::DockerRemoved(Box::new(outcome)));
                        }
                        #[cfg(not(feature = "permanent-delete"))]
                        Request::DockerRemove(_) => {
                            anyhow::bail!("removing is not available in this build");
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
