//! Throughput benchmark — **ticks/sec** of the headless sim, per representative
//! scenario. This is the version-to-version *comparator* for performance work:
//! because the sim is deterministic (seed + tick count ⇒ an identical workload,
//! SIM Law 10 / DEV Rule 3), a timing delta here is a real performance change, not
//! run-to-run drift. (To find *where* the time goes, profile instead — `flame`.)
//!
//! Compare two versions on the **same machine**:
//! ```text
//!   git checkout <old> && cargo bench --bench throughput -- --save-baseline old
//!   git checkout <new> && cargo bench --bench throughput -- --baseline old
//! ```
//! Criterion then prints the per-scenario % change ("Performance has improved").

use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use teemlab::SimConfig;

// Reuse the integration drivers' single-stepping harness *verbatim* rather than
// duplicate it: `stepping_app` builds the same headless world as the tests and
// both binaries (MinimalPlugins + SimPlugin, one `update()` = one fixed tick).
#[path = "../tests/common/mod.rs"]
mod common;
use common::stepping_app;

/// Scenarios chosen to exercise different hot paths: the general agent loop, the
/// MLP `decide` path, the nutrient field sub-pipeline, and interaction-heavy
/// predation. Each sustains a population across the measured window.
const SCENARIOS: &[&str] = &[
    "scenarios/examples/evolution.ron",
    "scenarios/examples/mlp_brain.ron",
    "scenarios/examples/nutrients.ron",
    "scenarios/examples/predator_prey.ron",
];

/// Untimed warm-up: step past the founding transient so we measure steady-state
/// work, not the first-tick spawn.
const WARMUP: usize = 64;
/// Fixed ticks measured per iteration (the determinism is what makes this stable).
const STEPS: u64 = 256;

fn throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("throughput");
    for &path in SCENARIOS {
        let cfg = SimConfig::from_ron_file(path)
            .unwrap_or_else(|e| panic!("bench scenario {path} unreadable: {e:?}"));
        let name = std::path::Path::new(path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(path);

        // `Throughput::Elements(STEPS)` makes Criterion report ticks/sec directly.
        group.throughput(Throughput::Elements(STEPS));
        group.bench_function(BenchmarkId::from_parameter(name), |b| {
            // PerIteration: rebuild+warm a fresh world for every measured run (the
            // rebuild is *untimed*). Determinism ⇒ identical warmed state each
            // time ⇒ constant measured work.
            b.iter_batched(
                || {
                    let mut app = stepping_app(&cfg);
                    for _ in 0..WARMUP {
                        app.update();
                    }
                    app
                },
                |mut app| {
                    for _ in 0..STEPS {
                        app.update();
                    }
                    app
                },
                BatchSize::PerIteration,
            );
        });
    }
    group.finish();
}

criterion_group!(benches, throughput);
criterion_main!(benches);
