//! The job runner: a run is owned here, not by a window.
//!
//! A run is a thread that works through the batches of a frozen plan in order.
//! Closing the window does not stop it (that is the point: the run window of
//! the next task only *watches*), so what keeps it honest is in this file:
//!
//! - **Cancel stops the queue and abandons the request in flight.** What
//!   already came back is kept: its batch is done, its proposals are written.
//!   A cancelled run is final.
//! - **A run stops when the app quits**, unless it is persisted as paused:
//!   [`Runner::pause_all`] (called on exit) turns every live run into a
//!   resumable `paused` one, and a record found `running` with no thread behind
//!   it (a crash) is reported as `paused` as well, never as running.
//! - **Running runs are listed** ([`Runner::running`]): a run that outlives its
//!   window spends money.
//! - **The destination is decided before anything is sent**: resolved once by
//!   [`client::destination`], refused when the project's `agent.backends`
//!   does not allow it (local only when that is empty).
//! - **An answer is only ever a proposal.** Each rule of a batch goes through
//!   `rules/validate`; the stored object is forced to `proposed = true` and
//!   `verified = false` whatever came back; nothing here writes
//!   `.rules-verdicts.json` or `.rules-waivers.json` (D1).
//!
//! The transport is one call, [`Client::ask`], behind this one seam: swapping
//! it for a queued transport (`AGENT_CHECKLIST_RUNNER.md`, variant B) touches
//! `ask_batch` and nothing else.
//!
//! Each run owns its record: the thread writes `run.json` after every step,
//! and the commands read the file, so there is no second copy in memory to
//! disagree with it.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{json, Value};

use super::client::{destination, AskError, AskRequest, Backend, Client, Destination, Usage};
use super::engine::{Engine, Plan, PlanRule};
use super::store::{
    self, ignore_state, request_key, rule_hashes, BatchRec, BatchState, CacheEntry, Freshness,
    IgnoreState, ProposalRec, Run, RunState, Store, Summary,
};
use super::{date_of, now_ms, Stop, StopKind};
use crate::apperr::AppError;

/// What ai-desktop is told it may take for one request. A batch is up to 60 KB
/// of files; a local model reads that slowly, a cloud one answers or fails on
/// its own gateway timeout well before this.
const ASK_TIMEOUT_LOCAL_MS: u64 = 600_000;
const ASK_TIMEOUT_CLOUD_MS: u64 = 180_000;
/// The longest `cancel` waits for the run's thread to write its final state.
const CANCEL_WAIT: Duration = Duration::from_secs(3);
/// The selection is a comma list of ids, families and gates; the engine
/// refuses more than 64 KiB, and this refuses it first.
const MAX_SELECTION: usize = 60_000;

/// What the dialog supplies to start a run.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartArgs {
    pub title: String,
    #[serde(default)]
    pub notes: String,
    /// Rule ids, families and `gate:<name>`, comma separated, as `rules/plan` takes them.
    pub selection: String,
    pub model: String,
    /// Ask again even where the answer is cached.
    #[serde(default)]
    pub refresh: bool,
}

/// One batch as the preview shows it (no prompt: that is the plan's bulk).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewBatch {
    pub id: i64,
    pub rules: Vec<PlanRule>,
    pub files: Vec<String>,
    pub bytes: u64,
    pub tokens: u64,
}

/// What a run would do, computed without sending anything: the destination
/// (resolved here, once), the batches, what is not sent and why, the totals.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    /// `None` when the project does not allow the model's backend; `refused` says why.
    pub destination: Option<Destination>,
    pub refused: Option<AppError>,
    /// `agent.models` of `.rules.json`, what the dialog offers.
    pub models: Vec<String>,
    pub batches: Vec<PreviewBatch>,
    pub skipped: Vec<Value>,
    pub totals: Value,
    /// Denied paths, limits, rules with a check, warnings: the engine's own.
    pub extra: Value,
    pub ignore: IgnoreState,
}

pub fn preview(
    engine: &dyn Engine,
    root: &Path,
    selection: &str,
    model: &str,
) -> Result<Preview, AppError> {
    let selection = check_selection(selection)?;
    let plan = engine.plan(root, selection)?;
    let (destination, refused) = match destination(model, &plan.agent.backends) {
        Ok(d) => (Some(d), None),
        Err(e) => (None, Some(e)),
    };
    Ok(Preview {
        destination,
        refused,
        models: plan.agent.models.clone(),
        batches: plan
            .batches
            .iter()
            .map(|b| PreviewBatch {
                id: b.id,
                rules: b.rules.clone(),
                files: b.files.clone(),
                bytes: b.bytes,
                tokens: b.tokens,
            })
            .collect(),
        skipped: plan.skipped.clone(),
        totals: plan.totals.clone(),
        extra: plan.extra.clone(),
        ignore: ignore_state(root),
    })
}

fn check_selection(selection: &str) -> Result<&str, AppError> {
    let s = selection.trim();
    if s.is_empty() || s.len() > MAX_SELECTION {
        return Err(AppError::new("err.agent.selection"));
    }
    Ok(s)
}

/// A run that has a thread.
struct Active {
    stop: Arc<Stop>,
    root: PathBuf,
    title: String,
    backend: Backend,
    model: String,
    started_ms: u64,
    handle: Option<JoinHandle<()>>,
}

/// A live run, for the list of what is spending money right now.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Running {
    pub id: String,
    pub root: String,
    pub title: String,
    pub backend: Backend,
    pub model: String,
    pub local: bool,
    pub started_ms: u64,
}

type ActiveMap = Arc<Mutex<HashMap<String, Active>>>;

/// Owns the live runs of the app. One instance, managed by Tauri.
#[derive(Default)]
pub struct Runner {
    active: ActiveMap,
}

/// What a run's thread needs besides the record.
#[derive(Clone)]
struct Ctx {
    engine: Arc<dyn Engine>,
    client: Client,
    root: PathBuf,
}

impl Runner {
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, HashMap<String, Active>>, AppError> {
        self.active.lock().map_err(|_| AppError::new("err.lock"))
    }

    /// Plan, check the destination, write the record, start the thread.
    ///
    /// Nothing is sent, and nothing is written, before the destination is
    /// allowed and the plan has something to ask.
    pub fn start(
        &self,
        root: &Path,
        args: StartArgs,
        engine: Arc<dyn Engine>,
        client: Client,
    ) -> Result<Run, AppError> {
        if args.title.trim().is_empty() {
            return Err(AppError::new("err.agent.title"));
        }
        let selection = check_selection(&args.selection)?.to_string();
        let plan = engine.plan(root, &selection)?;
        let dest = destination(&args.model, &plan.agent.backends)?;
        if plan.batches.is_empty() {
            return Err(AppError::new("err.agent.nothing").with("skipped", plan.skipped.len()));
        }

        let now = now_ms();
        let batches = plan
            .batches
            .iter()
            .map(|b| BatchRec {
                id: b.id,
                rules: b.rules.iter().map(|r| r.id.clone()).collect(),
                files: b.files.len(),
                bytes: b.bytes,
                tokens: b.tokens,
                key: request_key(&b.system, &b.prompt, dest.backend, &dest.model),
                ..Default::default()
            })
            .collect();
        let run = Run {
            id: store::new_id(),
            title: args.title.trim().to_string(),
            notes: args.notes,
            root: crate::portable(root),
            selection,
            backend: dest.backend,
            model: dest.model,
            local: dest.local,
            state: RunState::Running,
            created_ms: now,
            updated_ms: now,
            batches,
            skipped: plan.skipped.clone(),
            ..Default::default()
        };
        let store = Store::new(root);
        store.save_plan(&run.id, &plan)?;
        store.save(&run)?;
        let ctx = Ctx {
            engine,
            client,
            root: root.to_path_buf(),
        };
        self.launch(ctx, run.clone(), plan, args.refresh)?;
        Ok(run)
    }

    /// Continue a paused (or failed) run from its frozen plan: only the
    /// batches not done are asked, and a batch that failed is tried again.
    pub fn resume(
        &self,
        root: &Path,
        id: &str,
        engine: Arc<dyn Engine>,
        client: Client,
    ) -> Result<Run, AppError> {
        if self.lock()?.contains_key(id) {
            return Err(AppError::new("err.agent.running").with("id", id));
        }
        let store = Store::new(root);
        let mut run = store.load(id)?;
        let resumable = match run.state {
            RunState::Running | RunState::Paused | RunState::Failed => true,
            // Finished, but with batches that failed: a retry is a resume.
            RunState::Done => run.batches.iter().any(|b| b.state == BatchState::Failed),
            RunState::Cancelled => false,
        };
        if !resumable {
            return Err(AppError::new("err.agent.notResumable").with("id", id));
        }
        let plan = store.load_plan(id)?;
        for b in &mut run.batches {
            if matches!(b.state, BatchState::Failed | BatchState::Asking) {
                b.state = BatchState::Queued;
                b.error = None;
            }
        }
        run.state = RunState::Running;
        run.error = None;
        run.finished_ms = None;
        run.updated_ms = now_ms();
        store.save(&run)?;
        let ctx = Ctx {
            engine,
            client,
            root: root.to_path_buf(),
        };
        self.launch(ctx, run.clone(), plan, false)?;
        Ok(run)
    }

    fn launch(&self, ctx: Ctx, run: Run, plan: Plan, refresh: bool) -> Result<(), AppError> {
        let stop = Arc::new(Stop::default());
        let id = run.id.clone();
        self.lock()?.insert(
            id.clone(),
            Active {
                stop: stop.clone(),
                root: ctx.root.clone(),
                title: run.title.clone(),
                backend: run.backend,
                model: run.model.clone(),
                started_ms: now_ms(),
                handle: None,
            },
        );
        let map = self.active.clone();
        let thread_id = id.clone();
        let spawned = std::thread::Builder::new()
            .name("agent-run".into())
            .spawn(move || {
                execute(&ctx, run, &plan, &stop, refresh);
                // Off the list of live runs only after the record says what
                // became of it.
                if let Ok(mut live) = map.lock() {
                    live.remove(&thread_id);
                }
            });
        match spawned {
            Ok(h) => {
                if let Some(a) = self.lock()?.get_mut(&id) {
                    a.handle = Some(h);
                }
                Ok(())
            }
            Err(e) => {
                self.lock()?.remove(&id);
                Err(AppError::new("err.task").detail(e))
            }
        }
    }

    /// Stop a run: the queue ends and the request in flight is abandoned; what
    /// returned is kept. A run with no thread (paused, or found after a
    /// restart) is closed on disk. Cancelling a finished run changes nothing.
    pub fn cancel(&self, root: &Path, id: &str) -> Result<Run, AppError> {
        let store = Store::new(root);
        let live = {
            let mut map = self.lock()?;
            map.get_mut(id).map(|a| (a.stop.clone(), a.handle.take()))
        };
        if let Some((stop, handle)) = live {
            stop.request(StopKind::Cancel);
            if let Some(h) = handle {
                wait_for(&[h], CANCEL_WAIT);
            }
            return store.load(id);
        }
        let mut run = store.load(id)?;
        if run.state.is_open() {
            run.state = RunState::Cancelled;
            run.finished_ms = Some(now_ms());
            run.updated_ms = now_ms();
            for b in &mut run.batches {
                if b.state == BatchState::Asking {
                    b.state = BatchState::Queued;
                }
            }
            store.save(&run)?;
        }
        Ok(run)
    }

    /// The runs of a project, newest first. A run that says `running` with no
    /// thread behind it was cut off (a crash, a kill) and is `paused`.
    pub fn list(&self, root: &Path) -> Vec<Summary> {
        Store::new(root)
            .list()
            .iter()
            .map(|r| Summary::of(&self.as_seen(r.clone())))
            .collect()
    }

    /// One run in full.
    pub fn status(&self, root: &Path, id: &str) -> Result<Run, AppError> {
        Ok(self.as_seen(Store::new(root).load(id)?))
    }

    fn as_seen(&self, mut run: Run) -> Run {
        let live = self
            .active
            .lock()
            .map(|m| m.contains_key(&run.id))
            .unwrap_or(false);
        if run.state == RunState::Running && !live {
            run.state = RunState::Paused;
            for b in &mut run.batches {
                if b.state == BatchState::Asking {
                    b.state = BatchState::Queued;
                }
            }
        }
        run
    }

    /// Every run with a thread, in every project: what is spending money now.
    pub fn running(&self) -> Vec<Running> {
        let Ok(map) = self.active.lock() else {
            return Vec::new();
        };
        let mut out: Vec<Running> = map
            .iter()
            .map(|(id, a)| Running {
                id: id.clone(),
                root: crate::portable(&a.root),
                title: a.title.clone(),
                backend: a.backend,
                model: a.model.clone(),
                local: a.backend.is_local(),
                started_ms: a.started_ms,
            })
            .collect();
        out.sort_by(|a, b| a.started_ms.cmp(&b.started_ms).then(a.id.cmp(&b.id)));
        out
    }

    /// The app is quitting: every live run is paused (written as such and
    /// resumable) rather than left to die half-written. Waits up to `wait`
    /// for the threads to say so.
    pub fn pause_all(&self, wait: Duration) {
        let handles: Vec<JoinHandle<()>> = match self.active.lock() {
            Ok(mut map) => map
                .values_mut()
                .filter_map(|a| {
                    a.stop.request(StopKind::Pause);
                    a.handle.take()
                })
                .collect(),
            Err(_) => return,
        };
        wait_for(&handles, wait);
    }

    /// Whether each proposal of a run still matches the rules and the tree as
    /// they are now: plans the run's selection again (no network) and compares.
    pub fn freshness(
        &self,
        root: &Path,
        id: &str,
        engine: &dyn Engine,
    ) -> Result<BTreeMap<String, Freshness>, AppError> {
        let run = Store::new(root).load(id)?;
        let plan = engine.plan(root, &run.selection)?;
        Ok(store::outdated(&run, &plan))
    }
}

/// Wait for threads to end, up to `limit`.
fn wait_for(handles: &[JoinHandle<()>], limit: Duration) {
    let deadline = Instant::now() + limit;
    while handles.iter().any(|h| !h.is_finished()) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Write the record; a failure ends the run, since going on without a record
/// would spend money on answers that cannot be kept.
fn persist(store: &Store, run: &mut Run) -> bool {
    run.updated_ms = now_ms();
    match store.save(run) {
        Ok(()) => true,
        Err(e) => {
            run.error = Some(wire(&e));
            false
        }
    }
}

/// An error in the shape it has on the wire, to be kept in the record.
fn wire(e: &AppError) -> Value {
    serde_json::to_value(e).unwrap_or(Value::Null)
}

/// The one call that reaches the network. Everything about *which* request
/// (the text, the files, the rules) was decided by the engine's plan.
fn ask_batch(
    ctx: &Ctx,
    run: &Run,
    system: &str,
    prompt: &str,
    stop: &Stop,
) -> Result<super::client::Reply, AskError> {
    ctx.client.ask(
        &AskRequest {
            prompt: prompt.to_string(),
            system: system.to_string(),
            model: run.model.clone(),
            timeout_ms: if run.local {
                ASK_TIMEOUT_LOCAL_MS
            } else {
                ASK_TIMEOUT_CLOUD_MS
            },
        },
        stop,
    )
}

/// The proposal of one rule from the engine's object, or the error that kept
/// it from being checked. **Never a verdict**: `proposed` and `verified` are
/// set here whatever the engine said.
fn proposal_of(
    outcome: Result<Value, AppError>,
    rule: &str,
    batch: i64,
    key: &str,
    rule_hash: Option<String>,
) -> ProposalRec {
    let mut rec = ProposalRec {
        rule_id: rule.to_string(),
        batch,
        key: key.to_string(),
        rule_hash,
        created_ms: now_ms(),
        ..Default::default()
    };
    match outcome {
        Ok(mut p) => {
            if let Some(o) = p.as_object_mut() {
                o.insert("proposed".into(), Value::Bool(true));
                o.insert("verified".into(), Value::Bool(false));
            }
            rec.status = p
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("unclear")
                .to_string();
            rec.proposal = p;
        }
        Err(e) => {
            rec.status = "error".into();
            rec.proposal = json!({ "proposed": true, "verified": false });
            rec.error = Some(wire(&e));
        }
    }
    rec
}

/// Work through the queue of `run`. Returns when it is done, stopped, or
/// failed; the record on disk says which.
fn execute(ctx: &Ctx, mut run: Run, plan: &Plan, stop: &Stop, refresh: bool) {
    let store = Store::new(&ctx.root);
    let mut fatal = false;
    'queue: for i in 0..run.batches.len() {
        if matches!(run.batches[i].state, BatchState::Done | BatchState::Failed) {
            continue;
        }
        if stop.requested().is_some() {
            break;
        }
        let batch_id = run.batches[i].id;
        let Some(pb) = plan.batches.iter().find(|b| b.id == batch_id) else {
            run.batches[i].state = BatchState::Failed;
            run.batches[i].error = Some(wire(&AppError::new("err.agent.engineShape")));
            continue;
        };
        let key = run.batches[i].key.clone();
        run.batches[i].state = BatchState::Asking;
        run.batches[i].started_ms = Some(now_ms());
        if !persist(&store, &mut run) {
            fatal = true;
            break;
        }

        let cached = if refresh { None } else { store.cache_get(&key) };
        let (text, usage, from_cache) = match cached {
            Some(c) => (c.text, Usage::default(), true),
            None => match ask_batch(ctx, &run, &pb.system, &pb.prompt, stop) {
                Ok(reply) => {
                    store.cache_put(
                        &key,
                        &CacheEntry {
                            text: reply.text.clone(),
                            provider: run.backend.name().to_string(),
                            model: Some(run.model.clone()),
                            date: date_of(now_ms()),
                        },
                    );
                    (reply.text, reply.usage, false)
                }
                // Abandoned in flight: the batch is still to do.
                Err(AskError::Stopped(_)) => {
                    run.batches[i].state = BatchState::Queued;
                    break 'queue;
                }
                Err(e) => {
                    let stops_the_run = e.is_fatal();
                    let app = e.into_app(ctx.client.base());
                    let b = &mut run.batches[i];
                    b.state = BatchState::Failed;
                    b.finished_ms = Some(now_ms());
                    b.error = Some(wire(&app));
                    if stops_the_run {
                        run.error = Some(wire(&app));
                        fatal = true;
                    }
                    if !persist(&store, &mut run) {
                        fatal = true;
                    }
                    if fatal {
                        break 'queue;
                    }
                    continue;
                }
            },
        };

        // The request is answered (and paid for): it is kept even if a stop
        // came in meanwhile, so its rules are checked before the queue ends.
        let ids: Vec<String> = pb.rules.iter().map(|r| r.id.clone()).collect();
        let hashes = rule_hashes(&pb.prompt, &ids);
        for id in &ids {
            let outcome = ctx.engine.validate(&ctx.root, id, &text);
            run.set_proposal(proposal_of(
                outcome,
                id,
                batch_id,
                &key,
                hashes.get(id).cloned(),
            ));
        }
        let b = &mut run.batches[i];
        b.state = BatchState::Done;
        b.from_cache = from_cache;
        b.usage = usage;
        b.finished_ms = Some(now_ms());
        run.usage.add(usage);
        if !persist(&store, &mut run) {
            fatal = true;
            break;
        }
    }

    // A batch still marked as asking was abandoned: it is to do again.
    for b in &mut run.batches {
        if b.state == BatchState::Asking {
            b.state = BatchState::Queued;
        }
    }
    run.state = match stop.requested() {
        Some(StopKind::Cancel) => RunState::Cancelled,
        Some(StopKind::Pause) => RunState::Paused,
        None if fatal => RunState::Failed,
        None => RunState::Done,
    };
    if run.state != RunState::Paused {
        run.finished_ms = Some(now_ms());
    }
    let _ = persist(&store, &mut run);
}

#[cfg(test)]
mod tests {
    use super::super::engine::{AgentCfg, PlanBatch};
    use super::super::fake::{Fake, Resp};
    use super::*;
    use crate::testutil::fresh_dir;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// A plan of `n` batches, two rules each: `R<i>-1` and `R<i>-2`.
    fn plan_of(n: i64, backends: &[&str]) -> Plan {
        Plan {
            batches: (1..=n)
                .map(|i| PlanBatch {
                    id: i,
                    system: "SYSTEM".into(),
                    prompt: format!(
                        "Rules to check:\n\n### R{i}-1 (error)\nQuestion: q\nfirst rule {i}\n\n### R{i}-2 (warn)\nQuestion: q\nsecond rule {i}\n\nFiles:\n\n=== f{i}.lua ===\n1: code {i}"
                    ),
                    files: vec![format!("f{i}.lua")],
                    bytes: 10,
                    tokens: 5,
                    rules: vec![
                        PlanRule { id: format!("R{i}-1"), ..Default::default() },
                        PlanRule { id: format!("R{i}-2"), ..Default::default() },
                    ],
                })
                .collect(),
            agent: AgentCfg {
                backends: backends.iter().map(|s| s.to_string()).collect(),
                models: vec![],
            },
            ..Default::default()
        }
    }

    /// An engine with a canned plan whose validate mimics the real one's
    /// contract: the answer's JSON entry for the rule becomes the proposal,
    /// and anything else is `unclear` with the raw text.
    struct FakeEngine {
        plan: Mutex<Plan>,
        validated: Mutex<Vec<(String, String)>>,
        selections: Mutex<Vec<String>>,
        /// Claim `verified = true` in what comes back.
        lie: AtomicBool,
        /// The rule whose validation fails.
        fail: Mutex<Option<String>>,
    }

    impl FakeEngine {
        fn new(plan: Plan) -> Arc<FakeEngine> {
            Arc::new(FakeEngine {
                plan: Mutex::new(plan),
                validated: Mutex::new(Vec::new()),
                selections: Mutex::new(Vec::new()),
                lie: AtomicBool::new(false),
                fail: Mutex::new(None),
            })
        }
    }

    impl Engine for FakeEngine {
        fn plan(&self, _root: &Path, select: &str) -> Result<Plan, AppError> {
            self.selections.lock().unwrap().push(select.to_string());
            Ok(self.plan.lock().unwrap().clone())
        }

        fn validate(&self, _root: &Path, rule: &str, answer: &str) -> Result<Value, AppError> {
            self.validated
                .lock()
                .unwrap()
                .push((rule.to_string(), answer.to_string()));
            if self.fail.lock().unwrap().as_deref() == Some(rule) {
                return Err(AppError::new("err.agent.engine").detail("boom"));
            }
            let entry = serde_json::from_str::<Value>(answer)
                .ok()
                .and_then(|v| v.get("results").cloned())
                .and_then(|r| r.as_array()?.iter().find(|e| e["rule"] == rule).cloned());
            let lie = self.lie.load(Ordering::SeqCst);
            Ok(match entry {
                Some(e) => json!({
                    "rule_id": rule, "status": e["verdict"], "findings": [],
                    "proposed": !lie, "verified": lie,
                }),
                None => json!({
                    "rule_id": rule, "status": "unclear", "raw": answer,
                    "proposed": !lie, "verified": lie,
                }),
            })
        }
    }

    /// The model's answer to a prompt: a result for every rule it names.
    fn model_answer(prompt: &str) -> String {
        let results: Vec<Value> = prompt
            .lines()
            .filter_map(|l| l.strip_prefix("### "))
            .filter_map(|l| l.split(' ').next())
            .map(|id| json!({"rule": id, "verdict": if id.ends_with("-1") { "compliant" } else { "violation" }, "checked": []}))
            .collect();
        json!({ "results": results }).to_string()
    }

    fn ok_reply(prompt: &str) -> Resp {
        let body = json!({
            "text": model_answer(prompt), "provider": "ai_desktop",
            "usage": {"prompt_tokens": 10, "completion_tokens": 4}
        });
        Resp::json(200, &body.to_string())
    }

    fn prompt_of(seen: &super::super::fake::Seen) -> String {
        serde_json::from_str::<Value>(&seen.body).unwrap()["prompt"]
            .as_str()
            .unwrap()
            .to_string()
    }

    fn start_args(model: &str) -> StartArgs {
        StartArgs {
            title: "Release check".into(),
            notes: "for me".into(),
            selection: "R1".into(),
            model: model.into(),
            refresh: false,
        }
    }

    fn client_of(fake: &Fake) -> Client {
        Client::new(&fake.url())
            .unwrap()
            .with_slack(Duration::from_millis(300))
    }

    fn wait_state(runner: &Runner, root: &Path, id: &str, want: RunState) -> Run {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let run = runner.status(root, id).unwrap();
            if run.state == want {
                return run;
            }
            assert!(
                Instant::now() < deadline,
                "run stayed {:?}, wanted {want:?}",
                run.state
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn wait_asks(fake: &Fake, n: usize) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while fake.asks() < n {
            assert!(Instant::now() < deadline, "only {} asks", fake.asks());
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn wait_idle(runner: &Runner) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !runner.running().is_empty() {
            assert!(Instant::now() < deadline, "a run is still live");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn a_run_asks_each_batch_and_keeps_a_proposal_per_rule() {
        let root = fresh_dir("docmap-agent-run-ok");
        let fake = Fake::start(|s| ok_reply(&prompt_of(s)));
        let engine = FakeEngine::new(plan_of(2, &[]));
        let runner = Runner::default();

        let run = runner
            .start(
                &root,
                start_args("qwen2.5-coder"),
                engine.clone(),
                client_of(&fake),
            )
            .unwrap();
        assert_eq!(run.backend, Backend::Ollama);
        assert!(run.local);
        let done = wait_state(&runner, &root, &run.id, RunState::Done);

        assert_eq!(done.title, "Release check");
        assert_eq!(done.notes, "for me");
        assert_eq!(done.selection, "R1");
        assert_eq!(engine.selections.lock().unwrap().as_slice(), ["R1"]);
        assert_eq!(fake.asks(), 2);
        assert!(done
            .batches
            .iter()
            .all(|b| b.state == BatchState::Done && !b.from_cache));
        assert_eq!(
            done.usage,
            Usage {
                prompt_tokens: 20,
                completion_tokens: 8
            }
        );
        assert_eq!(done.proposals.len(), 4);
        let by = |id: &str| done.proposals.iter().find(|p| p.rule_id == id).unwrap();
        assert_eq!(by("R1-1").status, "compliant");
        assert_eq!(by("R2-2").status, "violation");
        assert!(by("R1-1").rule_hash.is_some());
        // The request carried what the plan said, to the model that was asked for.
        let first = serde_json::from_str::<Value>(&fake.seen()[0].body).unwrap();
        assert_eq!(first["model"], "qwen2.5-coder");
        assert_eq!(first["system"], "SYSTEM");
        assert!(first["prompt"].as_str().unwrap().contains("first rule 1"));
        // Each answer was handed to validate, once per rule.
        assert_eq!(engine.validated.lock().unwrap().len(), 4);
        wait_idle(&runner);
    }

    #[test]
    fn nothing_a_run_does_is_a_verdict_or_a_waiver() {
        let root = fresh_dir("docmap-agent-run-noverdict");
        let fake = Fake::start(|s| ok_reply(&prompt_of(s)));
        let engine = FakeEngine::new(plan_of(1, &[]));
        // An engine that claims the proposal is verified is not believed.
        engine.lie.store(true, Ordering::SeqCst);
        let runner = Runner::default();
        let run = runner
            .start(&root, start_args("llama3"), engine, client_of(&fake))
            .unwrap();
        let done = wait_state(&runner, &root, &run.id, RunState::Done);
        for p in &done.proposals {
            assert_eq!(p.proposal["proposed"], true, "{p:?}");
            assert_eq!(p.proposal["verified"], false, "{p:?}");
        }
        // Nothing but the proposals folder exists in the project.
        let names: Vec<String> = std::fs::read_dir(&root)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, [store::DIR]);
        wait_idle(&runner);
    }

    #[test]
    fn an_unchanged_rule_against_an_unchanged_tree_is_not_asked_twice() {
        let root = fresh_dir("docmap-agent-run-cache");
        let fake = Fake::start(|s| ok_reply(&prompt_of(s)));
        let engine = FakeEngine::new(plan_of(2, &[]));
        let runner = Runner::default();

        let a = runner
            .start(
                &root,
                start_args("llama3"),
                engine.clone(),
                client_of(&fake),
            )
            .unwrap();
        wait_state(&runner, &root, &a.id, RunState::Done);
        wait_idle(&runner);
        assert_eq!(fake.asks(), 2);

        let b = runner
            .start(
                &root,
                start_args("llama3"),
                engine.clone(),
                client_of(&fake),
            )
            .unwrap();
        let b = wait_state(&runner, &root, &b.id, RunState::Done);
        assert_eq!(fake.asks(), 2, "the second run is answered from the cache");
        assert!(b.batches.iter().all(|x| x.from_cache));
        assert_eq!(b.usage, Usage::default());
        assert_eq!(b.proposals.len(), 4, "and is still checked rule by rule");
        wait_idle(&runner);

        // Another model is another question; so is asking again on purpose.
        let c = runner
            .start(
                &root,
                start_args("mistral"),
                engine.clone(),
                client_of(&fake),
            )
            .unwrap();
        wait_state(&runner, &root, &c.id, RunState::Done);
        wait_idle(&runner);
        assert_eq!(fake.asks(), 4);
        let mut again = start_args("llama3");
        again.refresh = true;
        let d = runner
            .start(&root, again, engine, client_of(&fake))
            .unwrap();
        wait_state(&runner, &root, &d.id, RunState::Done);
        assert_eq!(fake.asks(), 6);
        wait_idle(&runner);
    }

    #[test]
    fn cancel_in_the_middle_of_a_batch_keeps_what_returned() {
        let root = fresh_dir("docmap-agent-run-cancel");
        // Batch 2 hangs for longer than the test would wait.
        let fake = Fake::start(|s| {
            let prompt = prompt_of(s);
            let r = ok_reply(&prompt);
            if prompt.contains("first rule 2") {
                r.after(30_000)
            } else {
                r
            }
        });
        let engine = FakeEngine::new(plan_of(3, &[]));
        let runner = Runner::default();
        let run = runner
            .start(&root, start_args("llama3"), engine, client_of(&fake))
            .unwrap();
        wait_asks(&fake, 2);
        assert_eq!(runner.running().len(), 1);

        let started = Instant::now();
        let cancelled = runner.cancel(&root, &run.id).unwrap();
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "{:?}",
            started.elapsed()
        );
        assert_eq!(cancelled.state, RunState::Cancelled);
        assert!(cancelled.finished_ms.is_some());
        let states: Vec<BatchState> = cancelled.batches.iter().map(|b| b.state).collect();
        assert_eq!(
            states,
            [BatchState::Done, BatchState::Queued, BatchState::Queued]
        );
        assert_eq!(cancelled.proposals.len(), 2, "batch 1 is kept");
        assert!(cancelled.proposals.iter().all(|p| p.batch == 1));

        wait_idle(&runner);
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(fake.asks(), 2, "batch 3 was never asked");
        // It stays cancelled, and cannot be resumed.
        assert_eq!(
            runner.status(&root, &run.id).unwrap().state,
            RunState::Cancelled
        );
        let e = runner
            .resume(
                &root,
                &run.id,
                FakeEngine::new(plan_of(3, &[])),
                client_of(&fake),
            )
            .unwrap_err();
        assert_eq!(e.key(), "err.agent.notResumable");
    }

    #[test]
    fn a_run_cut_off_by_a_restart_is_listed_paused_and_resumes_what_is_left() {
        let root = fresh_dir("docmap-agent-run-restart");
        let hang = Arc::new(AtomicBool::new(true));
        let h2 = hang.clone();
        let fake = Fake::start(move |s| {
            let prompt = prompt_of(s);
            let r = ok_reply(&prompt);
            if prompt.contains("first rule 2") && h2.load(Ordering::SeqCst) {
                r.after(30_000)
            } else {
                r
            }
        });
        let engine = FakeEngine::new(plan_of(3, &[]));
        let first = Runner::default();
        let run = first
            .start(
                &root,
                start_args("llama3"),
                engine.clone(),
                client_of(&fake),
            )
            .unwrap();
        wait_asks(&fake, 2);
        // The app quits while batch 2 is in flight.
        first.pause_all(Duration::from_secs(3));
        assert!(first.running().is_empty());
        let on_disk = Store::new(&root).load(&run.id).unwrap();
        assert_eq!(on_disk.state, RunState::Paused);
        assert_eq!(on_disk.batches[0].state, BatchState::Done);
        assert_eq!(on_disk.batches[1].state, BatchState::Queued);
        drop(first);

        // A new process: nothing in memory, the project is all it has.
        hang.store(false, Ordering::SeqCst);
        let second = Runner::default();
        let listed = second.list(&root);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, run.id);
        assert_eq!(listed[0].state, RunState::Paused);
        assert_eq!((listed[0].batches_done, listed[0].batches_total), (1, 3));
        assert_eq!(listed[0].proposals, 2);

        second
            .resume(&root, &run.id, engine, client_of(&fake))
            .unwrap();
        let done = wait_state(&second, &root, &run.id, RunState::Done);
        assert_eq!(fake.asks(), 4, "batch 1 was not asked again");
        assert_eq!(done.proposals.len(), 6);
        assert!(done.batches.iter().all(|b| b.state == BatchState::Done));
        wait_idle(&second);
    }

    #[test]
    fn a_record_left_running_by_a_crash_is_shown_as_paused() {
        let root = fresh_dir("docmap-agent-run-crash");
        let store = Store::new(&root);
        let mut run = Run {
            id: "r-crash".into(),
            state: RunState::Running,
            ..Default::default()
        };
        run.batches.push(BatchRec {
            state: BatchState::Asking,
            ..Default::default()
        });
        store.save(&run).unwrap();
        let runner = Runner::default();
        let seen = runner.status(&root, "r-crash").unwrap();
        assert_eq!(seen.state, RunState::Paused);
        assert_eq!(seen.batches[0].state, BatchState::Queued);
        assert_eq!(runner.list(&root)[0].state, RunState::Paused);
        // Cancelling a run with no thread closes it on disk.
        let c = runner.cancel(&root, "r-crash").unwrap();
        assert_eq!(c.state, RunState::Cancelled);
        assert_eq!(store.load("r-crash").unwrap().state, RunState::Cancelled);
        // And cancelling a finished run changes nothing.
        assert_eq!(
            runner.cancel(&root, "r-crash").unwrap().state,
            RunState::Cancelled
        );
    }

    #[test]
    fn a_cloud_model_is_refused_before_anything_is_sent_or_written() {
        let root = fresh_dir("docmap-agent-run-egress");
        let fake = Fake::start(|s| ok_reply(&prompt_of(s)));
        let runner = Runner::default();

        // The default is local only.
        let engine = FakeEngine::new(plan_of(1, &[]));
        let e = runner
            .start(
                &root,
                start_args("claude-sonnet-5"),
                engine.clone(),
                client_of(&fake),
            )
            .unwrap_err();
        assert_eq!(e.key(), "err.agent.localOnly");
        // Listing another cloud backend does not allow this one.
        let engine = FakeEngine::new(plan_of(1, &["openai"]));
        let e = runner
            .start(
                &root,
                start_args("claude-sonnet-5"),
                engine,
                client_of(&fake),
            )
            .unwrap_err();
        assert_eq!(e.key(), "err.agent.notAllowed");
        assert_eq!(fake.seen().len(), 0, "nothing reached ai-desktop");
        assert!(!root.join(store::DIR).exists(), "and nothing was written");

        // Allowed by the project: the run goes out and says where.
        let engine = FakeEngine::new(plan_of(1, &["anthropic"]));
        let run = runner
            .start(
                &root,
                start_args("claude-sonnet-5"),
                engine,
                client_of(&fake),
            )
            .unwrap();
        assert_eq!(run.backend, Backend::Anthropic);
        assert!(!run.local);
        assert_eq!(runner.running()[0].backend, Backend::Anthropic);
        wait_state(&runner, &root, &run.id, RunState::Done);
        wait_idle(&runner);
    }

    #[test]
    fn the_preview_resolves_the_destination_once_and_refuses_without_failing() {
        let root = fresh_dir("docmap-agent-run-preview");
        let engine = FakeEngine::new(plan_of(2, &[]));
        let p = preview(&*engine, &root, "R1", "gpt-5").unwrap();
        assert!(p.destination.is_none());
        assert_eq!(p.refused.as_ref().unwrap().key(), "err.agent.localOnly");
        assert_eq!(p.batches.len(), 2);
        assert_eq!(p.batches[0].files, ["f1.lua"]);
        assert!(!p.ignore.ignored);
        let p = preview(&*engine, &root, "R1", "qwen").unwrap();
        let d = p.destination.unwrap();
        assert_eq!((d.backend, d.local), (Backend::Ollama, true));
        assert_eq!(
            preview(&*engine, &root, "  ", "qwen").unwrap_err().key(),
            "err.agent.selection"
        );
        assert!(!root.join(store::DIR).exists(), "a preview writes nothing");
    }

    #[test]
    fn a_403_stops_the_run_and_keeps_the_queue_for_a_resume() {
        let root = fresh_dir("docmap-agent-run-403");
        let blocked = Arc::new(AtomicBool::new(true));
        let b2 = blocked.clone();
        let fake = Fake::start(move |s| {
            if b2.load(Ordering::SeqCst) {
                Resp::json(
                    403,
                    r#"{"error":{"message":"cross-origin request blocked"}}"#,
                )
            } else {
                ok_reply(&prompt_of(s))
            }
        });
        let engine = FakeEngine::new(plan_of(2, &[]));
        let runner = Runner::default();
        let run = runner
            .start(
                &root,
                start_args("llama3"),
                engine.clone(),
                client_of(&fake),
            )
            .unwrap();
        let failed = wait_state(&runner, &root, &run.id, RunState::Failed);
        assert_eq!(fake.asks(), 1, "the second batch would fail the same way");
        assert_eq!(failed.batches[0].state, BatchState::Failed);
        assert_eq!(failed.batches[1].state, BatchState::Queued);
        let err = failed.error.as_ref().unwrap();
        assert_eq!(err["key"], "err.agent.origin");
        assert!(failed.batches[0].error.is_some());
        wait_idle(&runner);

        blocked.store(false, Ordering::SeqCst);
        runner
            .resume(&root, &run.id, engine, client_of(&fake))
            .unwrap();
        let done = wait_state(&runner, &root, &run.id, RunState::Done);
        assert!(done.batches.iter().all(|b| b.state == BatchState::Done));
        assert!(done.error.is_none());
        wait_idle(&runner);
    }

    #[test]
    fn a_batch_the_backend_fails_does_not_stop_the_others() {
        let root = fresh_dir("docmap-agent-run-502");
        let fake = Fake::start(|s| {
            let p = prompt_of(s);
            if p.contains("first rule 1") {
                Resp::json(502, r#"{"error":{"message":"ollama: connection refused"}}"#)
            } else {
                ok_reply(&p)
            }
        });
        let engine = FakeEngine::new(plan_of(2, &[]));
        let runner = Runner::default();
        let run = runner
            .start(&root, start_args("llama3"), engine, client_of(&fake))
            .unwrap();
        let done = wait_state(&runner, &root, &run.id, RunState::Done);
        assert_eq!(done.batches[0].state, BatchState::Failed);
        assert_eq!(
            done.batches[0].error.as_ref().unwrap()["key"],
            "err.agent.backend"
        );
        assert_eq!(done.batches[1].state, BatchState::Done);
        assert_eq!(done.proposals.len(), 2);
        assert!(done.error.is_none());
        wait_idle(&runner);
    }

    #[test]
    fn a_non_schema_answer_is_an_unclear_proposal_with_the_raw_text() {
        let root = fresh_dir("docmap-agent-run-malformed");
        let fake = Fake::start(|_| {
            Resp::json(
                200,
                &json!({"text": "I think it is fine.", "usage": {}}).to_string(),
            )
        });
        let engine = FakeEngine::new(plan_of(1, &[]));
        let runner = Runner::default();
        let run = runner
            .start(
                &root,
                start_args("llama3"),
                engine.clone(),
                client_of(&fake),
            )
            .unwrap();
        let done = wait_state(&runner, &root, &run.id, RunState::Done);
        assert_eq!(done.proposals.len(), 2);
        for p in &done.proposals {
            assert_eq!(p.status, "unclear");
            assert_eq!(p.proposal["raw"], "I think it is fine.");
        }
        assert_eq!(engine.validated.lock().unwrap()[0].1, "I think it is fine.");
        wait_idle(&runner);
    }

    #[test]
    fn an_answer_ai_desktop_garbles_fails_the_batch_not_the_app() {
        let root = fresh_dir("docmap-agent-run-garbled");
        let fake = Fake::start(|_| Resp::json(200, "<html>proxy login</html>"));
        let engine = FakeEngine::new(plan_of(1, &[]));
        let runner = Runner::default();
        let run = runner
            .start(&root, start_args("llama3"), engine, client_of(&fake))
            .unwrap();
        let done = wait_state(&runner, &root, &run.id, RunState::Done);
        assert_eq!(done.batches[0].state, BatchState::Failed);
        assert_eq!(
            done.batches[0].error.as_ref().unwrap()["key"],
            "err.agent.malformed"
        );
        assert!(done.proposals.is_empty());
        wait_idle(&runner);
    }

    #[test]
    fn a_rule_the_engine_cannot_check_is_an_error_proposal_and_the_run_goes_on() {
        let root = fresh_dir("docmap-agent-run-validate-fails");
        let fake = Fake::start(|s| ok_reply(&prompt_of(s)));
        let engine = FakeEngine::new(plan_of(1, &[]));
        *engine.fail.lock().unwrap() = Some("R1-1".into());
        let runner = Runner::default();
        let run = runner
            .start(&root, start_args("llama3"), engine, client_of(&fake))
            .unwrap();
        let done = wait_state(&runner, &root, &run.id, RunState::Done);
        let bad = done.proposals.iter().find(|p| p.rule_id == "R1-1").unwrap();
        assert_eq!(bad.status, "error");
        assert_eq!(bad.error.as_ref().unwrap()["key"], "err.agent.engine");
        assert_eq!(bad.proposal["verified"], false);
        let good = done.proposals.iter().find(|p| p.rule_id == "R1-2").unwrap();
        assert_eq!(good.status, "violation");
        wait_idle(&runner);
    }

    #[test]
    fn an_edited_rule_shows_its_proposal_as_outdated() {
        let root = fresh_dir("docmap-agent-run-outdated");
        let fake = Fake::start(|s| ok_reply(&prompt_of(s)));
        let engine = FakeEngine::new(plan_of(1, &[]));
        let runner = Runner::default();
        let run = runner
            .start(
                &root,
                start_args("llama3"),
                engine.clone(),
                client_of(&fake),
            )
            .unwrap();
        wait_state(&runner, &root, &run.id, RunState::Done);
        wait_idle(&runner);

        let now = runner.freshness(&root, &run.id, &*engine).unwrap();
        assert!(now.values().all(|f| *f == Freshness::Current), "{now:?}");

        // The first rule's text is edited; the second is not.
        {
            let mut p = engine.plan.lock().unwrap();
            p.batches[0].prompt = p.batches[0]
                .prompt
                .replace("first rule 1", "first rule 1, EDITED");
        }
        let now = runner.freshness(&root, &run.id, &*engine).unwrap();
        assert_eq!(now["R1-1"], Freshness::Rule);
        assert_eq!(now["R1-2"], Freshness::Files);
    }

    #[test]
    fn a_plan_with_nothing_to_ask_is_refused_and_says_how_much_was_skipped() {
        let root = fresh_dir("docmap-agent-run-empty");
        let mut plan = plan_of(1, &[]);
        plan.batches.clear();
        plan.skipped = vec![json!({"id": "X-1", "reason": "no scope to look in"})];
        let fake = Fake::start(|_| Resp::json(200, "{}"));
        let e = Runner::default()
            .start(
                &root,
                start_args("llama3"),
                FakeEngine::new(plan),
                client_of(&fake),
            )
            .unwrap_err();
        assert_eq!(e.key(), "err.agent.nothing");
        assert_eq!(e.param("skipped"), Some("1"));
        let mut blank = start_args("llama3");
        blank.title = "  ".into();
        let e = Runner::default()
            .start(
                &root,
                blank,
                FakeEngine::new(plan_of(1, &[])),
                client_of(&fake),
            )
            .unwrap_err();
        assert_eq!(e.key(), "err.agent.title");
    }
}
