use std::sync::{Arc, atomic::AtomicBool};

use im::Vector;
use ltr_core::{EventInterpretation, EventReplay, StaticEvent, sync::Canceable};

#[derive(Debug, Default, Clone, PartialEq, Eq, Hash)]
pub struct MockPath {
    pub p: Vector<MockEvent>,
}

impl EventReplay for MockPath {
    type EventType = MockEvent;

    fn extend(&self, event: Self::EventType) -> Self {
        let mut clone = self.clone();
        clone.p.push_back(event);
        clone
    }

    fn is_prefix_of(&self, other: &Self) -> bool {
        if self.p.len() > other.p.len() {
            return false;
        }

        self.p.iter().zip(other.p.iter()).all(|(a, b)| a == b)
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MockEvent(pub bool);

impl StaticEvent for MockEvent {
    const VARIANTS: &'static [Self] = &[Self(true), Self(false)];
}

pub struct MockInterpretation(pub bool);

impl EventInterpretation for MockInterpretation {
    fn is_dead(&self) -> bool {
        !self.0
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
}
