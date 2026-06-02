use std::sync::{Arc, atomic::AtomicBool};

use im::Vector;
use ltr_core::{EventReplay, SelectionPolicy, StaticEvent, sync::Canceable};

#[derive(Debug, Default, Clone, PartialEq, Eq, Hash)]
pub struct MockPath {
    pub p: Vector<MockEvent>,
}

impl EventReplay for MockPath {
    type EventType = MockEvent;

    fn push(&mut self, event: Self::EventType) {
        self.p.push_back(event);
    }

    fn is_prefix_of(&self, other: &Self) -> bool {
        if self.p.len() > other.p.len() {
            return false;
        }

        self.p.iter().zip(other.p.iter()).all(|(a, b)| a == b)
    }

    fn extend_with_slice(&mut self, slice: &[Self::EventType]) {
        self.p.extend(slice.iter().cloned());
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MockEvent(pub bool);

impl StaticEvent for MockEvent {
    const VARIANTS: &'static [Self] = &[Self(true), Self(false)];
}

#[derive(Debug, Default, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
pub struct MockInterpretation(pub bool);

pub struct BooleanAcceptor;

impl SelectionPolicy<MockInterpretation> for BooleanAcceptor {
    fn compare(a: &MockInterpretation, b: &MockInterpretation) -> std::cmp::Ordering {
        a.cmp(b)
    }

    fn may_reject(s: &MockInterpretation) -> bool {
        !s.0
    }

    fn may_accept(s: &MockInterpretation) -> bool {
        s.0
    }
}

#[derive(Clone, Debug, Default)]
pub struct MockCancelToken {
    is_cancelled: Arc<AtomicBool>,
}

impl MockCancelToken {
    pub fn new() -> Self {
        Self {
            is_cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.is_cancelled.load(std::sync::atomic::Ordering::Acquire)
    }
}

impl Canceable for MockCancelToken {
    fn cancel(&self) {
        self.is_cancelled
            .store(true, std::sync::atomic::Ordering::Release);
    }

    fn is_cancelled(&self) -> bool {
        self.is_cancelled.load(std::sync::atomic::Ordering::Acquire)
    }
}
