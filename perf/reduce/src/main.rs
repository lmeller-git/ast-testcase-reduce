use ltr_schedule::BFScheduler;
use std::sync::Arc;
use std::time::Instant;

use reduce::*;

#[tokio::main]
async fn main() {
    let num_workers = 8;
    let query_len = 1000;

    let mut base_query = vec![0u16; query_len];

    let mut seed: u64 = 0xDEADBEEF;
    let mut next_random_idx = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed as usize) % query_len
    };

    for &val in CRITICAL_TOKENS {
        loop {
            let target_idx = next_random_idx();
            if !CRITICAL_TOKENS.contains(&base_query[target_idx]) {
                base_query[target_idx] = val;
                break;
            }
        }
    }

    println!("Starting Concurrent Minimizer Benchmark...");
    println!("Workers: {}", num_workers);
    println!("Query Length: {}", query_len);
    println!("Base query: {:?}", base_query);

    let start_time = Instant::now();
    let mut handles = Vec::new();
    let scheduler: Arc<
        BFScheduler<
            dry::MockPath,
            dry::MockEvent,
            MockCancelToken,
            MockInterpretationResult,
            MockResultInterpretor,
            2,
        >,
    > = Arc::new(BFScheduler::new());

    for _ in 0..num_workers {
        let sched_clone = scheduler.clone();
        let query_clone = base_query.clone();

        handles.push(tokio::spawn(async move {
            run_worker(sched_clone.clone(), query_clone.clone()).await;
        }));
    }
    for handle in handles {
        let _ = handle.await;
    }

    println!("Benchmark Complete in: {:.2?}", start_time.elapsed());
    println!("Minimized query: {:?}", MINIMAL.0.lock().unwrap());
}
