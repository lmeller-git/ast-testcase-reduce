use std::{
    hint::black_box,
    sync::{Arc, LazyLock, Mutex},
};

use dry::MockPath;
use ltr_core::{SelectionPolicy, sync::Canceable};
use ltr_schedule::StepScheduler;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug, Default)]
pub struct MockCancelToken {
    token: tokio_util::sync::CancellationToken,
}

impl MockCancelToken {
    pub fn new() -> Self {
        Self {
            token: CancellationToken::new(),
        }
    }

    pub fn token(&self) -> &CancellationToken {
        &self.token
    }
}

impl Canceable for MockCancelToken {
    fn cancel(&self) {
        self.token.cancel();
    }

    fn is_cancelled(&self) -> bool {
        self.token.is_cancelled()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MockInterpretationResult {
    Dead,
    Valid { length: usize },
}

pub struct MockResultInterpretor;

impl SelectionPolicy<MockInterpretationResult> for MockResultInterpretor {
    fn compare(a: &MockInterpretationResult, b: &MockInterpretationResult) -> std::cmp::Ordering {
        match (a, b) {
            (
                MockInterpretationResult::Valid { length: l1 },
                MockInterpretationResult::Valid { length: l2 },
            ) => l2.cmp(l1),
            (MockInterpretationResult::Dead, MockInterpretationResult::Dead) => {
                std::cmp::Ordering::Equal
            }
            (MockInterpretationResult::Valid { .. }, MockInterpretationResult::Dead) => {
                std::cmp::Ordering::Greater
            }
            (MockInterpretationResult::Dead, MockInterpretationResult::Valid { .. }) => {
                std::cmp::Ordering::Less
            }
        }
    }

    fn may_reject(s: &MockInterpretationResult) -> bool {
        matches!(s, MockInterpretationResult::Dead)
    }

    fn may_accept(_s: &MockInterpretationResult) -> bool {
        false
    }
}

#[derive(Default, Debug)]
pub struct Minimal(pub Mutex<Vec<u16>>);

pub static MINIMAL: LazyLock<Arc<Minimal>> = LazyLock::new(Arc::default);

pub const CRITICAL_TOKENS: &[u16] = &[
    4, 11, 22, 33, 78, 42, 90, 2342, 123, 22, 23444, 19829, 21333, 11, 1, 2183, 9902, 23947, 12843,
    23, 55, 1, 233,
];

pub fn algo(path: &MockPath, base_query: &mut Vec<u16>) {
    let mut kept_items = Vec::with_capacity(base_query.len());

    for (i, &val) in base_query.iter().enumerate() {
        let should_remove = path.p.get(i).map(|e| e.0).unwrap_or(false);
        if !should_remove {
            kept_items.push(val);
        }
    }

    *base_query = kept_items;
}

fn do_algo(path: &MockPath, mut base_query: Vec<u16>) -> MockInterpretationResult {
    if path.p.len() > base_query.len() {
        return MockInterpretationResult::Dead;
    }

    algo(path, &mut base_query);
    oracle(&base_query)
}

pub async fn run_worker<S>(scheduler: Arc<S>, base_query: Vec<u16>)
where
    S: StepScheduler<MockPath, MockCancelToken, StateInterpretation = MockInterpretationResult>,
    S::ItemMeta: Send + 'static + Clone,
{
    loop {
        let query = base_query.clone();
        let token = MockCancelToken::new();

        let Ok(path) = scheduler.next(token.clone()) else {
            return;
        };

        let (tx, rx) = tokio::sync::oneshot::channel();
        let path_ = path.path().clone();

        tokio::spawn(async move {
            let res = tokio::task::spawn_blocking(move || do_algo(&path_, query)).await;
            if let Ok(interpretation) = res {
                let _ = tx.send(interpretation);
            }
        });

        tokio::select! {
            _ = token.token().cancelled() => {
                // Cancelled by scheduler
            }
            Ok(interpretation) = rx => {
                scheduler.put_result(path.clone(), interpretation);
            }
        }
    }
}

pub fn oracle(query: &[u16]) -> MockInterpretationResult {
    let mut x: i32 = 0;
    for _ in 0..5_000_000 {
        x = x.wrapping_add(1);
        black_box(x);
    }

    let res = CRITICAL_TOKENS.iter().all(|token| query.contains(token));
    if res {
        let mut min = MINIMAL.0.lock().unwrap();
        if min.len() > query.len() || min.is_empty() {
            *min = query.to_vec();
        }
        return MockInterpretationResult::Valid {
            length: query.len(),
        };
    }
    MockInterpretationResult::Dead
}
