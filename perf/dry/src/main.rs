use std::hint::black_box;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Instant;

use dry::{MockCancelToken, MockInterpretation, MockPath};
use ltr_schedule::{BFScheduler, StepScheduler};

// benchmarks llm generated

#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

fn mock_oracle(path: &MockPath) -> bool {
    std::thread::sleep(std::time::Duration::from_micros(10));
    let n_true = path.p.iter().filter(|x| x.0).count();
    (n_true + 1) % 2 == 0
}

fn main() {
    #[cfg(feature = "dhat-heap")]
    let _profiler = dhat::Profiler::new_heap();

    println!("Starting 500k Query Scheduler Stress Test...");
    run_stress_test(1, 500_000);
    run_stress_test(5, 500_000);
    run_stress_test(16, 500_000);
}

fn run_stress_test(num_workers: usize, total_queries: usize) {
    let scheduler = Arc::new(BFScheduler::new());
    let global_counter = Arc::new(AtomicUsize::new(0));

    let now = Instant::now();

    thread::scope(|scope| {
        for _ in 0..num_workers {
            let sch = scheduler.clone();
            let counter = global_counter.clone();

            scope.spawn(move || {
                while counter.fetch_add(1, Ordering::Relaxed) < total_queries {
                    let token = MockCancelToken::new();
                    if let Ok(path) = sch.next(token.clone()) {
                        if token.is_cancelled() {
                            continue;
                        }
                        let is_valid = mock_oracle(path.path());
                        sch.put_result(path, MockInterpretation(is_valid));
                    } else {
                        break;
                    }
                }
            });
        }
    });

    let duration = now.elapsed();

    black_box(scheduler);

    let qps = total_queries as f64 / duration.as_secs_f64();
    println!(
        "Workers: {:2} | Time: {:.4}s | Queries/Sec: {:.2}",
        num_workers,
        duration.as_secs_f64(),
        qps
    );
}
