//! Store path uploader.
//!
//! There are two APIs: `Pusher` and `PushSession`.
//!
//! A `Pusher` simply dispatches `ValidPathInfo`s for workers to push. Use this
//! when you know all store paths to push beforehand. The push plan (closure, missing
//! paths, all path metadata) should be computed prior to pushing.
//!
//! A `PushSession`, on the other hand, accepts a stream of `StorePath`s and
//! takes care of retrieving the closure and path metadata. It automatically
//! batches expensive operations (closure computation, querying missing paths).
//! Use this when the list of store paths is streamed from some external
//! source (e.g., FS watcher, Unix Domain Socket) and a push plan cannot be
//! created statically.
//!
//! TODO: Refactor out progress reporting and support a simple output style without progress bars

use std::collections::{HashMap, HashSet};
use std::fmt::Write;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context as _, Result};
use async_channel as channel;
use bytes::Bytes;
use futures::future::join_all;
use futures::stream::Stream;
use indicatif::{HumanBytes, MultiProgress, ProgressBar, ProgressState, ProgressStyle};
use tokio::sync::{mpsc, Mutex};
use tokio::task::{spawn, JoinHandle};
use tokio::time;

use crate::api::ApiClient;
use attic::api::v1::cache_config::CacheConfig;
use attic::api::v1::upload_path::{UploadPathNarInfo, UploadPathResult, UploadPathResultKind};
use attic::cache::CacheName;
use attic::error::AtticResult;
use attic::nix_store::{NixStore, StorePath, StorePathHash, ValidPathInfo};
use tracing::error;

type JobSender = channel::Sender<PushJob>;
type JobReceiver = channel::Receiver<PushJob>;

/// A unit of work for a push worker.
enum PushJob {
    /// Upload the NAR, then record a build trace if the path needs one.
    Upload(ValidPathInfo),

    /// Record only the build trace, for a path the cache already holds.
    RecordTrace { path: StorePath, deriver: String },
}

/// Configuration for pushing store paths.
#[derive(Clone, Copy, Debug)]
pub struct PushConfig {
    /// The number of workers to spawn.
    pub num_workers: usize,

    /// Whether to always include the upload info in the PUT payload.
    pub force_preamble: bool,
}

/// Configuration for a push session.
#[derive(Clone, Copy, Debug)]
pub struct PushSessionConfig {
    /// Push the specified paths only and do not compute closures.
    pub no_closure: bool,

    /// Ignore the upstream cache filter.
    pub ignore_upstream_cache_filter: bool,
}

/// A handle to push store paths to a cache.
///
/// The caller is responsible for computing closures and
/// checking for paths that already exist on the remote
/// cache.
pub struct Pusher {
    api: ApiClient,
    store: Arc<NixStore>,
    cache: CacheName,
    cache_config: CacheConfig,
    workers: Vec<JoinHandle<HashMap<StorePath, Result<()>>>>,
    sender: JobSender,
}

/// A wrapper over a `Pusher` that accepts a stream of `StorePath`s.
///
/// Unlike a `Pusher`, a `PushSession` takes a stream of `StorePath`s
/// instead of `ValidPathInfo`s, taking care of retrieving the closure
/// and path metadata.
///
/// This is useful when the list of store paths is streamed from some
/// external source (e.g., FS watcher, Unix Domain Socket) and a push
/// plan cannot be computed statically.
///
/// ## Batching
///
/// Many store paths can be built in a short period of time, with each
/// having a big closure. It can be very inefficient if we were to compute
/// closure and query for missing paths for each individual path. This is
/// especially true if we have a lot of remote builders (e.g., `attic watch-store`
/// running alongside a beefy Hydra instance).
///
/// `PushSession` batches operations in order to minimize the number of
/// closure computations and API calls. It also remembers which paths already
/// exist on the remote cache. By default, it submits a batch if it's been 2
/// seconds since the last path is queued or it's been 10 seconds in total.
pub struct PushSession {
    /// Sender to the batching future.
    sender: channel::Sender<SessionQueueCommand>,

    /// Receiver of results.
    result_receiver: mpsc::Receiver<Result<HashMap<StorePath, Result<()>>>>,
}

enum SessionQueueCommand {
    Paths(Vec<StorePath>),
    Flush,
    Terminate,
}

enum SessionQueuePoll {
    Paths(Vec<StorePath>),
    Flush,
    Terminate,
    Closed,
    TimedOut,
}

#[derive(Debug)]
pub struct PushPlan {
    /// Store paths to push.
    pub store_path_map: HashMap<StorePathHash, ValidPathInfo>,

    /// Cached paths whose build trace still needs recording.
    ///
    /// A cache can hold the NAR while the trace is still missing.
    pub traces_to_record: Vec<(StorePath, String)>,

    /// The number of paths in the original full closure.
    pub num_all_paths: usize,

    /// Number of paths that have been filtered out because they are already cached.
    pub num_already_cached: usize,

    /// Number of paths that have been filtered out because they are signed by an upstream cache.
    pub num_upstream: usize,
}

/// Wrapper to update a progress bar as a NAR is streamed.
struct NarStreamProgress<S> {
    stream: S,
    bar: ProgressBar,
}

impl Pusher {
    pub fn new(
        store: Arc<NixStore>,
        api: ApiClient,
        cache: CacheName,
        cache_config: CacheConfig,
        mp: MultiProgress,
        config: PushConfig,
    ) -> Self {
        let (sender, receiver) = channel::unbounded();
        let mut workers = Vec::new();

        for _ in 0..config.num_workers {
            workers.push(spawn(Self::worker(
                receiver.clone(),
                store.clone(),
                api.clone(),
                cache.clone(),
                mp.clone(),
                config,
            )));
        }

        Self {
            api,
            store,
            cache,
            cache_config,
            workers,
            sender,
        }
    }

    /// Queues a store path to be pushed.
    pub async fn queue(&self, path_info: ValidPathInfo) -> Result<()> {
        self.send(PushJob::Upload(path_info)).await
    }

    /// Queues a path for build-trace recording only.
    ///
    /// The push plan skips paths the cache already holds, so they would
    /// otherwise never get a trace.
    pub async fn queue_trace_only(&self, path: StorePath, deriver: String) -> Result<()> {
        self.send(PushJob::RecordTrace { path, deriver }).await
    }

    async fn send(&self, job: PushJob) -> Result<()> {
        self.sender.send(job).await.map_err(|e| anyhow!(e))
    }

    /// Waits for all workers to terminate, returning all results.
    ///
    /// TODO: Stream the results with another channel
    pub async fn wait(self) -> HashMap<StorePath, Result<()>> {
        drop(self.sender);

        let results = join_all(self.workers)
            .await
            .into_iter()
            .map(|joinresult| joinresult.unwrap())
            .fold(HashMap::new(), |mut acc, results| {
                acc.extend(results);
                acc
            });

        results
    }

    /// Creates a push plan.
    pub async fn plan(
        &self,
        roots: Vec<StorePath>,
        no_closure: bool,
        ignore_upstream_filter: bool,
    ) -> Result<PushPlan> {
        PushPlan::plan(
            self.store.clone(),
            &self.api,
            &self.cache,
            &self.cache_config,
            roots,
            no_closure,
            ignore_upstream_filter,
        )
        .await
    }

    /// Converts the pusher into a `PushSession`.
    ///
    /// This is useful when the list of store paths is streamed from some
    /// external source (e.g., FS watcher, Unix Domain Socket) and a push
    /// plan cannot be computed statically.
    pub fn into_push_session(self, config: PushSessionConfig) -> PushSession {
        PushSession::with_pusher(self, config)
    }

    async fn worker(
        receiver: JobReceiver,
        store: Arc<NixStore>,
        api: ApiClient,
        cache: CacheName,
        mp: MultiProgress,
        config: PushConfig,
    ) -> HashMap<StorePath, Result<()>> {
        let mut results = HashMap::new();

        loop {
            let job = match receiver.recv().await {
                Ok(job) => job,
                Err(_) => {
                    // channel is closed - we are done
                    break;
                }
            };

            let (store_path, r) = match job {
                PushJob::Upload(path_info) => {
                    let store_path = path_info.path.clone();
                    let r = upload_path(
                        path_info,
                        store.clone(),
                        api.clone(),
                        &cache,
                        mp.clone(),
                        config.force_preamble,
                    )
                    .await;
                    (store_path, r)
                }
                // A trace failure only warns, so this arm is always `Ok`.
                PushJob::RecordTrace { path, deriver } => {
                    record_build_trace_or_warn(&path, &deriver, &store, &api, &cache, &mp).await;
                    (path, Ok(()))
                }
            };

            results.insert(store_path, r);
        }

        results
    }
}

impl PushSession {
    pub fn with_pusher(pusher: Pusher, config: PushSessionConfig) -> Self {
        let (sender, receiver) = channel::unbounded();
        let (result_sender, result_receiver) = mpsc::channel(1);

        let known_paths_mutex = Arc::new(Mutex::new(HashSet::new()));

        spawn(async move {
            if let Err(e) = Self::worker(
                pusher,
                config,
                known_paths_mutex.clone(),
                receiver.clone(),
                result_sender.clone(),
            )
            .await
            {
                error!("Push session worker died: {e:#}");
                let _ = result_sender.send(Err(e)).await;
            }
        });

        Self {
            sender,
            result_receiver,
        }
    }

    async fn worker(
        pusher: Pusher,
        config: PushSessionConfig,
        known_paths_mutex: Arc<Mutex<HashSet<StorePathHash>>>,
        receiver: channel::Receiver<SessionQueueCommand>,
        result_sender: mpsc::Sender<Result<HashMap<StorePath, Result<()>>>>,
    ) -> Result<()> {
        let mut roots = HashSet::new();

        loop {
            // Get outstanding paths in queue
            let done = tokio::select! {
                // 2 seconds since last queued path
                done = async {
                    loop {
                        let poll = tokio::select! {
                            r = receiver.recv() => match r {
                                Ok(SessionQueueCommand::Paths(paths)) => SessionQueuePoll::Paths(paths),
                                Ok(SessionQueueCommand::Flush) => SessionQueuePoll::Flush,
                                Ok(SessionQueueCommand::Terminate) => SessionQueuePoll::Terminate,
                                _ => SessionQueuePoll::Closed,
                            },
                            _ = time::sleep(Duration::from_secs(2)) => SessionQueuePoll::TimedOut,
                        };

                        match poll {
                            SessionQueuePoll::Paths(store_paths) => {
                                roots.extend(store_paths.into_iter());
                            }
                            SessionQueuePoll::Closed | SessionQueuePoll::Terminate => {
                                break true;
                            }
                            SessionQueuePoll::Flush | SessionQueuePoll::TimedOut => {
                                break false;
                            }
                        }
                    }
                } => done,

                // 10 seconds
                _ = time::sleep(Duration::from_secs(10)) => {
                    false
                },
            };

            // Compute push plan
            let roots_vec: Vec<StorePath> = {
                let known_paths = known_paths_mutex.lock().await;
                roots
                    .drain()
                    .filter(|root| !known_paths.contains(&root.to_hash()))
                    .collect()
            };

            let mut plan = pusher
                .plan(
                    roots_vec,
                    config.no_closure,
                    config.ignore_upstream_cache_filter,
                )
                .await
                .context("Failed to compute push plan")?;

            let mut known_paths = known_paths_mutex.lock().await;
            plan.store_path_map
                .retain(|sph, _| !known_paths.contains(sph));
            // A batch recomputes the plan from scratch, so without this every
            // already-cached path is re-queued on every batch, forever.
            plan.traces_to_record
                .retain(|(path, _)| !known_paths.contains(&path.to_hash()));

            // Push everything
            for (store_path_hash, path_info) in plan.store_path_map.into_iter() {
                pusher
                    .queue(path_info)
                    .await
                    .context("Failed to queue path for upload")?;
                known_paths.insert(store_path_hash);
            }

            for (path, deriver) in plan.traces_to_record {
                let store_path_hash = path.to_hash();
                pusher
                    .queue_trace_only(path, deriver)
                    .await
                    .context("Failed to queue path for build trace recording")?;
                known_paths.insert(store_path_hash);
            }

            drop(known_paths);

            if done {
                let result = pusher.wait().await;
                result_sender.send(Ok(result)).await?;
                return Ok(());
            }
        }
    }

    /// Waits for all workers to terminate, returning all results.
    pub async fn wait(mut self) -> Result<HashMap<StorePath, Result<()>>> {
        self.flush()?;

        // The worker might have died
        let _ = self.sender.send(SessionQueueCommand::Terminate).await;

        self.result_receiver
            .recv()
            .await
            .expect("Nothing in result channel")
    }

    /// Queues multiple store paths to be pushed.
    pub fn queue_many(&self, store_paths: Vec<StorePath>) -> Result<()> {
        self.sender
            .send_blocking(SessionQueueCommand::Paths(store_paths))
            .map_err(|e| anyhow!(e))
    }

    /// Flushes the worker queue.
    pub fn flush(&self) -> Result<()> {
        self.sender
            .send_blocking(SessionQueueCommand::Flush)
            .map_err(|e| anyhow!(e))
    }
}

impl PushPlan {
    /// Creates a plan.
    async fn plan(
        store: Arc<NixStore>,
        api: &ApiClient,
        cache: &CacheName,
        cache_config: &CacheConfig,
        roots: Vec<StorePath>,
        no_closure: bool,
        ignore_upstream_filter: bool,
    ) -> Result<Self> {
        let roots = {
            let mut result = vec![];
            for root in roots {
                if store.is_valid_path(root.clone()).await? {
                    result.push(root);
                } else {
                    eprintln!("Ignoring {:?}", root);
                }
            }
            result
        };

        // Compute closure
        let closure = if no_closure {
            roots
        } else {
            store.compute_fs_closure_multi(roots, false).await?
        };

        let mut store_path_map: HashMap<StorePathHash, ValidPathInfo> = {
            let futures = closure
                .iter()
                .map(|path| {
                    let store = store.clone();
                    let path = path.clone();
                    let path_hash = path.to_hash();

                    async move {
                        // The watch logic is not infallible and we can get non-existent paths here.
                        let opt_path_info = store.query_path_info(path).await?;
                        Ok(opt_path_info.map(|path_info| (path_hash, path_info)))
                    }
                })
                .collect::<Vec<_>>();

            join_all(futures)
                .await
                .into_iter()
                .filter_map(|r| r.transpose())
                .collect::<Result<_>>()?
        };

        let num_all_paths = store_path_map.len();
        if store_path_map.is_empty() {
            return Ok(Self {
                store_path_map,
                traces_to_record: Vec::new(),
                num_all_paths,
                num_already_cached: 0,
                num_upstream: 0,
            });
        }

        if !ignore_upstream_filter {
            // Filter out paths signed by upstream caches
            let upstream_cache_key_names = cache_config
                .upstream_cache_key_names
                .as_ref()
                .map_or([].as_slice(), |v| v.as_slice());
            store_path_map.retain(|_, pi| {
                for sig in &pi.sigs {
                    if let Some((name, _)) = sig.split_once(':') {
                        if upstream_cache_key_names.iter().any(|u| name == u) {
                            return false;
                        }
                    }
                }

                true
            });
        }

        let num_filtered_paths = store_path_map.len();
        if store_path_map.is_empty() {
            return Ok(Self {
                store_path_map,
                traces_to_record: Vec::new(),
                num_all_paths,
                num_already_cached: 0,
                num_upstream: num_all_paths - num_filtered_paths,
            });
        }

        // Query missing paths
        let missing_path_hashes: HashSet<StorePathHash> = {
            let store_path_hashes = store_path_map.keys().map(|sph| sph.to_owned()).collect();
            let res = api.get_missing_paths(cache, store_path_hashes).await?;
            res.missing_paths.into_iter().collect()
        };
        // A trace is a separate fact from the NAR, so an already-cached path
        // may still be missing one.
        let (missing, already_cached): (HashMap<_, _>, HashMap<_, _>) = store_path_map
            .into_iter()
            .partition(|(sph, _)| missing_path_hashes.contains(sph));

        store_path_map = missing;

        // This drops every path without a content address or a deriver, so a
        // caller can queue each remaining entry without checking again.
        let traces_to_record = already_cached
            .into_values()
            .filter(|pi| pi.ca.is_some())
            .filter_map(|pi| Some((pi.path, pi.deriver?)))
            .collect();

        let num_missing_paths = store_path_map.len();

        Ok(Self {
            store_path_map,
            traces_to_record,
            num_all_paths,
            num_already_cached: num_filtered_paths - num_missing_paths,
            num_upstream: num_all_paths - num_filtered_paths,
        })
    }
}

/// Uploads a single path to a cache.
pub async fn upload_path(
    path_info: ValidPathInfo,
    store: Arc<NixStore>,
    api: ApiClient,
    cache: &CacheName,
    mp: MultiProgress,
    force_preamble: bool,
) -> Result<()> {
    let path = &path_info.path;
    let is_content_addressed = path_info.ca.is_some();
    let deriver = path_info.deriver.clone();
    let upload_info = {
        let full_path = store
            .get_full_path(path)
            .to_str()
            .ok_or_else(|| anyhow!("Path contains non-UTF-8"))?
            .to_string();

        let references = path_info
            .references
            .into_iter()
            .map(|pb| {
                pb.as_os_str()
                    .to_str()
                    .ok_or_else(|| anyhow!("Reference contains non-UTF-8"))
                    .map(|s| s.to_owned())
            })
            .collect::<Result<Vec<String>, anyhow::Error>>()?;

        UploadPathNarInfo {
            cache: cache.to_owned(),
            store_path_hash: path.to_hash(),
            store_path: full_path,
            references,
            system: None,  // TODO
            deriver: None, // TODO
            sigs: path_info.sigs,
            ca: path_info.ca,
            nar_hash: path_info.nar_hash.to_owned(),
            nar_size: path_info.nar_size as usize,
        }
    };

    let template = format!(
        "{{spinner}} {: <20.20} {{bar:40.green/blue}} {{human_bytes:10}} ({{average_speed}})",
        path.name(),
    );
    let style = ProgressStyle::with_template(&template)
        .unwrap()
        .tick_chars("🕛🕐🕑🕒🕓🕔🕕🕖🕗🕘🕙🕚✅")
        .progress_chars("██ ")
        .with_key("human_bytes", |state: &ProgressState, w: &mut dyn Write| {
            write!(w, "{}", HumanBytes(state.pos())).unwrap();
        })
        // Adapted from
        // <https://github.com/console-rs/indicatif/issues/394#issuecomment-1309971049>
        .with_key(
            "average_speed",
            |state: &ProgressState, w: &mut dyn Write| match (state.pos(), state.elapsed()) {
                (pos, elapsed) if elapsed > Duration::ZERO => {
                    write!(w, "{}", average_speed(pos, elapsed)).unwrap();
                }
                _ => write!(w, "-").unwrap(),
            },
        );
    let bar = mp.add(ProgressBar::new(path_info.nar_size));
    bar.set_style(style);
    let nar_stream = NarStreamProgress::new(store.nar_from_path(path.to_owned()), bar.clone());

    let start = Instant::now();
    match api
        .upload_path(upload_info, nar_stream, force_preamble)
        .await
    {
        Ok(r) => {
            let r = r.unwrap_or(UploadPathResult {
                kind: UploadPathResultKind::Uploaded,
                file_size: None,
                frac_deduplicated: None,
            });

            let info_string: String = match r.kind {
                UploadPathResultKind::Deduplicated => "deduplicated".to_string(),
                _ => {
                    let elapsed = start.elapsed();
                    let seconds = elapsed.as_secs_f64();
                    let speed = (path_info.nar_size as f64 / seconds) as u64;

                    let mut s = format!(
                        "{}, {}/s",
                        HumanBytes(path_info.nar_size),
                        HumanBytes(speed)
                    );

                    if let Some(frac_deduplicated) = r.frac_deduplicated {
                        if frac_deduplicated > 0.01f64 {
                            s += &format!(", {:.1}% deduplicated", frac_deduplicated * 100.0);
                        }
                    }

                    s
                }
            };

            mp.suspend(|| {
                eprintln!(
                    "✅ {} ({})",
                    path.as_os_str().to_string_lossy(),
                    info_string
                );
            });

            bar.finish_and_clear();

            if is_content_addressed {
                if let Some(deriver) = deriver.as_deref() {
                    record_build_trace_or_warn(path, deriver, &store, &api, cache, &mp).await;
                }
            }

            Ok(())
        }
        Err(e) => {
            mp.suspend(|| {
                eprintln!("❌ {}: {}", path.as_os_str().to_string_lossy(), e);
            });
            bar.finish_and_clear();
            Err(e)
        }
    }
}

/// Records a path's build trace, reporting a failure as a warning.
async fn record_build_trace_or_warn(
    path: &StorePath,
    deriver: &str,
    store: &NixStore,
    api: &ApiClient,
    cache: &CacheName,
    mp: &MultiProgress,
) {
    if let Err(e) = upload_build_trace(path, deriver, store, api, cache).await {
        mp.suspend(|| {
            eprintln!(
                "⚠️ {}: failed to record build trace: {}",
                path.as_os_str().to_string_lossy(),
                e
            );
        });
    }
}

/// Records the build trace covering a content-addressed path.
async fn upload_build_trace(
    path: &StorePath,
    deriver: &str,
    store: &NixStore,
    api: &ApiClient,
    cache: &CacheName,
) -> Result<()> {
    let base_name = path
        .as_os_str()
        .to_str()
        .ok_or_else(|| anyhow!("Path contains non-UTF-8"))?;

    // `ca` is set for fixed-output derivations too, and only the store can
    // tell those from a floating output.
    let Some(output) = store.floating_output(path, deriver).await? else {
        return Ok(());
    };

    api.put_build_trace(cache, &output.drv_name, &output.output_name, base_name)
        .await
}

impl<S: Stream<Item = AtticResult<Bytes>>> NarStreamProgress<S> {
    fn new(stream: S, bar: ProgressBar) -> Self {
        Self { stream, bar }
    }
}

impl<S: Stream<Item = AtticResult<Bytes>> + Unpin> Stream for NarStreamProgress<S> {
    type Item = AtticResult<Bytes>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match Pin::new(&mut self.stream).as_mut().poll_next(cx) {
            Poll::Ready(Some(data)) => {
                if let Ok(data) = &data {
                    self.bar.inc(data.len() as u64);
                }

                Poll::Ready(Some(data))
            }
            other => other,
        }
    }
}

// Just the average, no fancy sliding windows that cause wild fluctuations
// <https://github.com/console-rs/indicatif/issues/394>
fn average_speed(bytes: u64, duration: Duration) -> String {
    let speed = bytes as f64 * 1000_f64 / duration.as_millis() as f64;
    format!("{}/s", HumanBytes(speed as u64))
}
