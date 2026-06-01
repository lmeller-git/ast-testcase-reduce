use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use dry::{MockCancelToken, MockInterpretation, MockPath};
use ltr_schedule::{BFScheduler, StepScheduler};
use std::hint::black_box;
use std::sync::Arc;
use std::thread;

// benchmarks llm generated

fn mock_oracle(path: &MockPath) -> bool {
    std::thread::sleep(std::time::Duration::from_micros(10));
    let n_true = path.p.iter().filter(|x| x.0).count();
    (n_true + 1) % 2 == 0
}

fn bench_scheduler_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("scheduler_throughput");
    let operations_per_iter = 1000;
    group.throughput(Throughput::Elements(operations_per_iter as u64));

    group.bench_function("sequential_1_worker", |b| {
        b.iter_with_setup(
            || Arc::new(BFScheduler::new()),
            |scheduler| {
                for _ in 0..operations_per_iter {
                    let token = MockCancelToken::new();
                    if let Ok(path) = scheduler.next(token.clone()) {
                        let is_valid = mock_oracle(path.path());
                        let event_interp = MockInterpretation(is_valid);
                        scheduler.put_result(path, event_interp);
                    } else {
                        break;
                    }
                }
                black_box(scheduler);
            },
        );
    });

    group.bench_function("parallel_5_workers", |b| {
        b.iter_with_setup(
            || Arc::new(BFScheduler::new()),
            |scheduler| {
                let num_workers = 5;

                thread::scope(|scope| {
                    for _ in 0..num_workers {
                        let sched = scheduler.clone();
                        scope.spawn(move || {
                            for _ in 0..(operations_per_iter / num_workers) {
                                let token = MockCancelToken::new();
                                if let Ok(path) = sched.next(token.clone()) {
                                    if token.is_cancelled() {
                                        continue;
                                    }

                                    let is_valid = mock_oracle(path.path());
                                    let event_interp = MockInterpretation(is_valid);

                                    sched.put_result(path, event_interp);
                                } else {
                                    break;
                                }
                            }
                        });
                    }
                });
                black_box(scheduler);
            },
        );
    });

    group.finish();
}

criterion_group!(benches, bench_scheduler_throughput);
criterion_main!(benches);
