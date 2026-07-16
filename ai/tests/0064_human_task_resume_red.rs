use std::fs;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use ai::adapters::outbound::{HumanTaskFileStore, SystemHumanTaskTimeFormatter};
use ai::application::{
    HumanTaskCoordinator, HumanTaskParentInput, HumanTaskResume, HumanTaskResumeError,
    HumanTaskStatus,
};
use ai::domain::human_task_checkpoint::*;
use ai::ports::outbound::*;
use aibe_protocol::{
    HandoffExecutionOutcome, HumanTaskBriefing, HumanTaskRequest, PostHandoffObservation,
    ShellLogRange,
};

fn task() -> HumanTaskRequest {
    HumanTaskRequest {
        objective: "review deployment".into(),
        reason: None,
        instructions: vec!["inspect".into()],
        completion_criteria: vec!["report".into()],
    }
}

fn observation(cwd: &Path, marker: &str) -> PostHandoffObservation {
    PostHandoffObservation {
        cwd_exists: true,
        cwd: cwd.display().to_string(),
        git_head: Some(marker.into()),
        git_branch: Some("main".into()),
        git_status: Some("clean".into()),
        shell_log_tail: Some(marker.into()),
        shell_log_truncated: Some(false),
        observation_errors: vec![],
        human_task_evidence: None,
    }
}

fn suspended_checkpoint(cwd: PathBuf) -> HumanTaskCheckpointV1 {
    HumanTaskCheckpointV1 {
        version: 1,
        task_id: HumanTaskId::parse("ht-20260714-7f31c2").unwrap(),
        state: HumanTaskWorkflowState::Suspended,
        task: task(),
        parent: HumanTaskParentContext {
            ai_session_id: "s1".into(),
            conversation_id: "c1".into(),
            turn_id: "t1".into(),
            user_request: "please review".into(),
            original_cwd: cwd.clone(),
            llm_profile: "fast".into(),
        },
        created_at_ms: 10,
        updated_at_ms: 20,
        suspended_at_ms: Some(20),
        suspend_reason: Some("need approval".into()),
        current_cwd: cwd.clone(),
        segments: vec![HumanShellSegment {
            index: 0,
            shell_session_id: "shell-1".into(),
            started_at_ms: 10,
            ended_at_ms: 20,
            initial_cwd: cwd.clone(),
            final_cwd: cwd.clone(),
            shell_log_range: ShellLogRange {
                start: 2,
                end: Some(9),
            },
            observation: observation(&cwd, "seg0"),
            end_reason: HumanShellSegmentEnd::Suspended,
        }],
        final_result: None,
        continuation: HumanTaskContinuationState::default(),
    }
}

struct Identity {
    now: AtomicU64,
}
impl HumanTaskIdentity for Identity {
    fn new_task_id(&self) -> HumanTaskId {
        HumanTaskId::parse("ht-20260714-7f31c2").unwrap()
    }
    fn now_ms(&self) -> u64 {
        self.now.fetch_add(10, Ordering::SeqCst)
    }
}

struct Observer {
    marker: Mutex<String>,
}
impl EnvironmentObserver for Observer {
    fn observe(
        &self,
        cwd: &Path,
        _: u64,
        _: Option<u64>,
        _: Option<&Path>,
    ) -> PostHandoffObservation {
        observation(cwd, &self.marker.lock().unwrap())
    }
}

#[derive(Clone)]
enum LaunchPlan {
    Suspend { reason: String, final_cwd: PathBuf },
    Done { final_cwd: PathBuf },
    Fail,
}

struct ScriptedLauncher {
    log: Arc<Mutex<Vec<HumanShellLaunchRequest>>>,
    plans: Mutex<Vec<LaunchPlan>>,
    lock_path: Option<PathBuf>,
}
impl HumanShellLauncher for ScriptedLauncher {
    fn launch_and_wait(
        &self,
        request: &HumanShellLaunchRequest,
        _: &AtomicBool,
    ) -> Result<HumanShellReturn, HumanShellLaunchError> {
        if let Some(lock_path) = &self.lock_path {
            let lock = fs::File::open(lock_path).unwrap();
            assert_eq!(
                unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
                -1,
                "root lock must be held during resume shell"
            );
        }
        self.log.lock().unwrap().push(request.clone());
        let plan = self.plans.lock().unwrap().pop().unwrap_or(LaunchPlan::Fail);
        match plan {
            LaunchPlan::Suspend { reason, final_cwd } => Err(HumanShellLaunchError::Suspended {
                returned: Box::new(HumanShellReturn {
                    outcome: HumanShellOutcome::Suspended,
                    suspend_reason: Some(reason.clone()),
                    exit_code: Some(0),
                    final_cwd,
                    shell_session_id: "shell-resume".into(),
                    shell_session_dir: PathBuf::new(),
                    shell_log_start: 10,
                    shell_log_end: 20,
                }),
                reason: Some(reason),
            }),
            LaunchPlan::Done { final_cwd } => Ok(HumanShellReturn {
                outcome: HumanShellOutcome::Done,
                suspend_reason: None,
                exit_code: Some(0),
                final_cwd,
                shell_session_id: "shell-done".into(),
                shell_session_dir: PathBuf::new(),
                shell_log_start: 10,
                shell_log_end: 20,
            }),
            LaunchPlan::Fail => Err(HumanShellLaunchError::Failed("boom".into())),
        }
    }
}

fn parent(cwd: &Path) -> HumanTaskParentInput {
    HumanTaskParentInput {
        ai_session_id: "s1".into(),
        conversation_id: "c1".into(),
        turn_id: "t1".into(),
        user_request: "please review".into(),
        cwd: cwd.to_path_buf(),
        llm_profile: "fast".into(),
        runtime_dir: cwd.join("runtime"),
    }
}

#[test]
fn human_task_resume_vertical_e2e() {
    let dir = tempfile::tempdir().unwrap();
    let store = HumanTaskFileStore::new(dir.path().into());
    let cwd = dir.path().to_path_buf();
    store.save(&suspended_checkpoint(cwd.clone())).unwrap();

    let launches = Arc::new(Mutex::new(Vec::new()));
    let launcher = ScriptedLauncher {
        log: launches.clone(),
        plans: Mutex::new(vec![LaunchPlan::Suspend {
            reason: "vpn next".into(),
            final_cwd: cwd.clone(),
        }]),
        lock_path: None,
    };
    let identity = Identity {
        now: AtomicU64::new(100),
    };
    let observer = Observer {
        marker: Mutex::new("seg1".into()),
    };
    let message = HumanTaskResume::new(&store, &identity, &launcher, &observer)
        .execute(None, cwd.join("runtime-resume"), &AtomicBool::new(false))
        .unwrap();
    assert!(message.contains("ht-20260714-7f31c2"));
    assert!(message.contains("ai human-task resume"));

    let loaded = store.load_active().unwrap();
    assert_eq!(loaded.state, HumanTaskWorkflowState::Suspended);
    assert_eq!(loaded.segments.len(), 2);
    assert_eq!(loaded.segments[1].index, 1);
    assert_eq!(loaded.suspend_reason.as_deref(), Some("vpn next"));
    assert_eq!(
        loaded.segments[1].observation.shell_log_tail.as_deref(),
        Some("seg1")
    );

    let status = HumanTaskStatus::new(&store, &SystemHumanTaskTimeFormatter)
        .render()
        .unwrap();
    assert!(status.contains("State: suspended"));
    assert!(status.contains("Resume:\n  ai human-task resume"));
    assert!(status.contains("Cancel:\n  ai human-task cancel --yes"));
}

#[test]
fn human_task_resume_restores_cwd_and_briefing() {
    let dir = tempfile::tempdir().unwrap();
    let store = HumanTaskFileStore::new(dir.path().into());
    let cwd = dir.path().join("work");
    fs::create_dir_all(&cwd).unwrap();
    store.save(&suspended_checkpoint(cwd.clone())).unwrap();

    let launches = Arc::new(Mutex::new(Vec::new()));
    let launcher = ScriptedLauncher {
        log: launches.clone(),
        plans: Mutex::new(vec![LaunchPlan::Suspend {
            reason: "again".into(),
            final_cwd: cwd.clone(),
        }]),
        lock_path: None,
    };
    HumanTaskResume::new(
        &store,
        &Identity {
            now: AtomicU64::new(50),
        },
        &launcher,
        &Observer {
            marker: Mutex::new("x".into()),
        },
    )
    .execute(None, dir.path().join("rt"), &AtomicBool::new(false))
    .unwrap();

    let request = launches.lock().unwrap().pop().unwrap();
    assert_eq!(request.cwd, cwd);
    assert_eq!(
        request.task_briefing,
        Some(HumanTaskBriefing::from(&task()))
    );
}

#[test]
fn human_task_resume_appends_suspended_segment() {
    let dir = tempfile::tempdir().unwrap();
    let store = HumanTaskFileStore::new(dir.path().into());
    let cwd = dir.path().to_path_buf();
    store.save(&suspended_checkpoint(cwd.clone())).unwrap();
    let launcher = ScriptedLauncher {
        log: Arc::new(Mutex::new(Vec::new())),
        plans: Mutex::new(vec![LaunchPlan::Suspend {
            reason: "second".into(),
            final_cwd: cwd.join("next"),
        }]),
        lock_path: None,
    };
    fs::create_dir_all(cwd.join("next")).unwrap();
    HumanTaskResume::new(
        &store,
        &Identity {
            now: AtomicU64::new(30),
        },
        &launcher,
        &Observer {
            marker: Mutex::new("seg1".into()),
        },
    )
    .execute(None, cwd.join("rt"), &AtomicBool::new(false))
    .unwrap();
    let loaded = store.load_active().unwrap();
    assert_eq!(loaded.segments.len(), 2);
    assert_eq!(
        loaded.segments[0].observation.git_head.as_deref(),
        Some("seg0")
    );
    assert_eq!(loaded.segments[1].index, 1);
    assert_eq!(loaded.current_cwd, cwd.join("next"));
}

#[test]
fn human_task_resume_holds_root_lock() {
    let dir = tempfile::tempdir().unwrap();
    let store = HumanTaskFileStore::new(dir.path().into());
    let cwd = dir.path().to_path_buf();
    store.save(&suspended_checkpoint(cwd.clone())).unwrap();
    let launcher = ScriptedLauncher {
        log: Arc::new(Mutex::new(Vec::new())),
        plans: Mutex::new(vec![LaunchPlan::Suspend {
            reason: "locked".into(),
            final_cwd: cwd.clone(),
        }]),
        lock_path: Some(dir.path().join("human-tasks/lock")),
    };
    HumanTaskResume::new(
        &store,
        &Identity {
            now: AtomicU64::new(40),
        },
        &launcher,
        &Observer {
            marker: Mutex::new("seg1".into()),
        },
    )
    .execute(None, cwd.join("rt"), &AtomicBool::new(false))
    .unwrap();
    let lock = store.lock_exclusive().unwrap();
    drop(lock);
}

#[test]
fn human_task_status_shows_resume_command() {
    let dir = tempfile::tempdir().unwrap();
    let store = HumanTaskFileStore::new(dir.path().into());
    store
        .save(&suspended_checkpoint(dir.path().into()))
        .unwrap();
    let status = HumanTaskStatus::new(&store, &SystemHumanTaskTimeFormatter)
        .render()
        .unwrap();
    assert!(status.contains("Resume:\n  ai human-task resume\n"));
    assert!(status.contains("Cancel:\n  ai human-task cancel --yes\n"));

    let fixed = format!(
        "Human Task suspended.\n\nTask:\n  {}\n\nResume:\n  ai human-task resume\n\nCancel:\n  ai human-task cancel --yes\n",
        "ht-20260714-7f31c2"
    );
    assert!(fixed.contains("Resume:\n  ai human-task resume"));
}

#[test]
fn human_task_resume_supports_multiple_suspends() {
    let dir = tempfile::tempdir().unwrap();
    let store = HumanTaskFileStore::new(dir.path().into());
    let cwd = dir.path().to_path_buf();
    store.save(&suspended_checkpoint(cwd.clone())).unwrap();
    for (index, marker) in ["seg1", "seg2"].into_iter().enumerate() {
        let launcher = ScriptedLauncher {
            log: Arc::new(Mutex::new(Vec::new())),
            plans: Mutex::new(vec![LaunchPlan::Suspend {
                reason: format!("r{index}"),
                final_cwd: cwd.clone(),
            }]),
            lock_path: None,
        };
        HumanTaskResume::new(
            &store,
            &Identity {
                now: AtomicU64::new(100 + index as u64 * 20),
            },
            &launcher,
            &Observer {
                marker: Mutex::new(marker.into()),
            },
        )
        .execute(
            None,
            cwd.join(format!("rt{index}")),
            &AtomicBool::new(false),
        )
        .unwrap();
    }
    let loaded = store.load_active().unwrap();
    assert_eq!(loaded.segments.len(), 3);
    assert_eq!(
        loaded
            .segments
            .iter()
            .map(|s| s.observation.shell_log_tail.clone())
            .collect::<Vec<_>>(),
        vec![
            Some("seg0".into()),
            Some("seg1".into()),
            Some("seg2".into())
        ]
    );
}

#[test]
fn human_task_resume_rejects_missing_or_mismatched_id() {
    let dir = tempfile::tempdir().unwrap();
    let store = HumanTaskFileStore::new(dir.path().into());
    let launcher = ScriptedLauncher {
        log: Arc::new(Mutex::new(Vec::new())),
        plans: Mutex::new(vec![]),
        lock_path: None,
    };
    let identity = Identity {
        now: AtomicU64::new(1),
    };
    let observer = Observer {
        marker: Mutex::new("x".into()),
    };
    assert_eq!(
        HumanTaskResume::new(&store, &identity, &launcher, &observer).execute(
            None,
            dir.path().join("rt"),
            &AtomicBool::new(false)
        ),
        Err(HumanTaskResumeError::NotFound)
    );

    store
        .save(&suspended_checkpoint(dir.path().into()))
        .unwrap();
    let before = fs::read(
        dir.path()
            .join("human-tasks/ht-20260714-7f31c2/checkpoint.json"),
    )
    .unwrap();
    assert_eq!(
        HumanTaskResume::new(&store, &identity, &launcher, &observer).execute(
            Some("ht-20260714-000001"),
            dir.path().join("rt"),
            &AtomicBool::new(false)
        ),
        Err(HumanTaskResumeError::NotFound)
    );
    let after = fs::read(
        dir.path()
            .join("human-tasks/ht-20260714-7f31c2/checkpoint.json"),
    )
    .unwrap();
    assert_eq!(before, after);
    assert!(launcher.log.lock().unwrap().is_empty());
}

#[test]
fn human_task_resume_rejects_non_suspended() {
    let dir = tempfile::tempdir().unwrap();
    let store = HumanTaskFileStore::new(dir.path().into());
    let mut running = suspended_checkpoint(dir.path().into());
    running.state = HumanTaskWorkflowState::Running;
    running.suspended_at_ms = None;
    running.suspend_reason = None;
    running.segments.clear();
    store.save(&running).unwrap();
    let launcher = ScriptedLauncher {
        log: Arc::new(Mutex::new(Vec::new())),
        plans: Mutex::new(vec![]),
        lock_path: None,
    };
    assert_eq!(
        HumanTaskResume::new(
            &store,
            &Identity {
                now: AtomicU64::new(1)
            },
            &launcher,
            &Observer {
                marker: Mutex::new("x".into())
            }
        )
        .execute(None, dir.path().join("rt"), &AtomicBool::new(false)),
        Err(HumanTaskResumeError::NotSuspended)
    );
    assert!(launcher.log.lock().unwrap().is_empty());
}

#[test]
fn human_task_resume_cwd_unavailable_fails_closed() {
    let dir = tempfile::tempdir().unwrap();
    let store = HumanTaskFileStore::new(dir.path().into());
    let missing = dir.path().join("gone");
    store.save(&suspended_checkpoint(missing)).unwrap();
    let before = fs::read(
        dir.path()
            .join("human-tasks/ht-20260714-7f31c2/checkpoint.json"),
    )
    .unwrap();
    let launcher = ScriptedLauncher {
        log: Arc::new(Mutex::new(Vec::new())),
        plans: Mutex::new(vec![]),
        lock_path: None,
    };
    assert_eq!(
        HumanTaskResume::new(
            &store,
            &Identity {
                now: AtomicU64::new(1)
            },
            &launcher,
            &Observer {
                marker: Mutex::new("x".into())
            }
        )
        .execute(None, dir.path().join("rt"), &AtomicBool::new(false)),
        Err(HumanTaskResumeError::CwdUnavailable)
    );
    let after = fs::read(
        dir.path()
            .join("human-tasks/ht-20260714-7f31c2/checkpoint.json"),
    )
    .unwrap();
    assert_eq!(before, after);
    assert!(launcher.log.lock().unwrap().is_empty());
}

#[test]
fn human_task_resume_done_restores_suspended() {
    let dir = tempfile::tempdir().unwrap();
    let store = HumanTaskFileStore::new(dir.path().into());
    let cwd = dir.path().to_path_buf();
    let original = suspended_checkpoint(cwd.clone());
    store.save(&original).unwrap();
    let launcher = ScriptedLauncher {
        log: Arc::new(Mutex::new(Vec::new())),
        plans: Mutex::new(vec![LaunchPlan::Done {
            final_cwd: cwd.clone(),
        }]),
        lock_path: None,
    };
    assert_eq!(
        HumanTaskResume::new(
            &store,
            &Identity {
                now: AtomicU64::new(80)
            },
            &launcher,
            &Observer {
                marker: Mutex::new("should-not-persist".into())
            }
        )
        .execute(None, cwd.join("rt"), &AtomicBool::new(false)),
        Err(HumanTaskResumeError::CompletionDeferred)
    );
    let restored = store.load_active().unwrap();
    assert_eq!(restored.state, HumanTaskWorkflowState::Suspended);
    assert_eq!(restored.segments.len(), 1);
    assert_eq!(restored.suspend_reason, original.suspend_reason);
    assert_eq!(
        restored.segments[0].observation.shell_log_tail.as_deref(),
        Some("seg0")
    );
}

#[test]
fn human_task_resume_preserves_single_segment_regression() {
    let dir = tempfile::tempdir().unwrap();
    let store = HumanTaskFileStore::new(dir.path().into());
    let cwd = dir.path().to_path_buf();
    let launches = Arc::new(Mutex::new(Vec::new()));
    let launcher = ScriptedLauncher {
        log: launches,
        plans: Mutex::new(vec![LaunchPlan::Suspend {
            reason: "first".into(),
            final_cwd: cwd.clone(),
        }]),
        lock_path: None,
    };
    let result = HumanTaskCoordinator::new(
        &store,
        &Identity {
            now: AtomicU64::new(10),
        },
        &launcher,
        &Observer {
            marker: Mutex::new("seg0".into()),
        },
    )
    .execute(task(), parent(&cwd), &AtomicBool::new(false));
    assert_eq!(result.status, HandoffExecutionOutcome::Suspended);
    assert_eq!(store.load_active().unwrap().segments.len(), 1);

    let done_launcher = ScriptedLauncher {
        log: Arc::new(Mutex::new(Vec::new())),
        plans: Mutex::new(vec![LaunchPlan::Done {
            final_cwd: cwd.clone(),
        }]),
        lock_path: None,
    };
    let done_store = HumanTaskFileStore::new(dir.path().join("done-history").into());
    fs::create_dir_all(dir.path().join("done-history")).unwrap();
    let done = HumanTaskCoordinator::new(
        &done_store,
        &Identity {
            now: AtomicU64::new(10),
        },
        &done_launcher,
        &Observer {
            marker: Mutex::new("done".into()),
        },
    )
    .execute(task(), parent(&cwd), &AtomicBool::new(false));
    assert_eq!(done.status, HandoffExecutionOutcome::Done);
    assert_eq!(done_store.load_active(), Err(HumanTaskStoreError::NotFound));
}
