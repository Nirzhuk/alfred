//! Tracks in-flight runs so the UI can cancel them and kill CLI children.

use std::collections::HashMap;
use std::process::Child;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use serde::Serialize;

#[derive(Clone)]
pub struct RunControl {
    pub cancel: Arc<AtomicBool>,
    pub(crate) child: Arc<Mutex<Option<Child>>>,
}

impl RunControl {
    pub fn new() -> Self {
        Self {
            cancel: Arc::new(AtomicBool::new(false)),
            child: Arc::new(Mutex::new(None)),
        }
    }

    /// Branch-local control sharing the run's cancellation flag.
    ///
    /// Parallel fan-out gives each branch its own child slot (concurrent CLI
    /// children would otherwise overwrite one shared slot and leak kills),
    /// while `cancel` stays shared so a user cancel — or a sibling's
    /// fail-fast abort — stops every branch through the existing polling
    /// loops.
    pub fn fork(&self) -> Self {
        Self {
            cancel: Arc::clone(&self.cancel),
            child: Arc::new(Mutex::new(None)),
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    pub fn request_cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
        if let Ok(mut slot) = self.child.lock() {
            if let Some(mut child) = slot.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }

    pub fn set_child(&self, child: Child) {
        if let Ok(mut slot) = self.child.lock() {
            *slot = Some(child);
        }
    }

    pub fn take_child(&self) -> Option<Child> {
        self.child.lock().ok().and_then(|mut slot| slot.take())
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveRun {
    pub run_id: String,
    pub workflow_id: String,
    pub workflow_name: String,
}

struct Entry {
    control: RunControl,
    meta: ActiveRun,
}

fn registry() -> &'static Mutex<HashMap<String, Entry>> {
    static REGISTRY: OnceLock<Mutex<HashMap<String, Entry>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn register(run_id: &str, workflow_id: &str, workflow_name: &str) -> RunControl {
    let control = RunControl::new();
    let meta = ActiveRun {
        run_id: run_id.to_string(),
        workflow_id: workflow_id.to_string(),
        workflow_name: workflow_name.to_string(),
    };
    if let Ok(mut map) = registry().lock() {
        map.insert(
            run_id.to_string(),
            Entry {
                control: control.clone(),
                meta,
            },
        );
    }
    control
}

pub fn get(run_id: &str) -> Option<RunControl> {
    registry()
        .lock()
        .ok()
        .and_then(|map| map.get(run_id).map(|e| e.control.clone()))
}

pub fn list_active() -> Vec<ActiveRun> {
    registry()
        .lock()
        .map(|map| map.values().map(|e| e.meta.clone()).collect())
        .unwrap_or_default()
}

pub fn has_workflow(workflow_id: &str) -> bool {
    registry()
        .lock()
        .map(|map| {
            map.values()
                .any(|entry| entry.meta.workflow_id == workflow_id)
        })
        .unwrap_or(false)
}

pub fn unregister(run_id: &str) {
    if let Ok(mut map) = registry().lock() {
        map.remove(run_id);
    }
}

pub fn cancel(run_id: &str) -> bool {
    if let Some(control) = get(run_id) {
        control.request_cancel();
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_runs_for_multiple_workflows_independently() {
        let suffix = uuid::Uuid::new_v4().to_string();
        let first_run = format!("run-a-{suffix}");
        let second_run = format!("run-b-{suffix}");
        let first_workflow = format!("workflow-a-{suffix}");
        let second_workflow = format!("workflow-b-{suffix}");

        register(&first_run, &first_workflow, "First");
        register(&second_run, &second_workflow, "Second");

        assert!(has_workflow(&first_workflow));
        assert!(has_workflow(&second_workflow));
        let active = list_active();
        assert!(active.iter().any(|run| run.run_id == first_run));
        assert!(active.iter().any(|run| run.run_id == second_run));

        unregister(&first_run);
        assert!(!has_workflow(&first_workflow));
        assert!(has_workflow(&second_workflow));
        unregister(&second_run);
    }
}
