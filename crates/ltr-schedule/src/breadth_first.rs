use std::{
    collections::VecDeque,
    hash::Hash,
    marker::PhantomData,
    sync::{Mutex, atomic::AtomicU64},
};

use ltr_core::{EventInterpretation, EventReplay, ScheduledStep, StaticEvent, sync::Canceable};
use smallvec::SmallVec;

use crate::StepScheduler;

#[derive(Hash, Clone, Default, Debug, PartialEq, Eq)]
pub struct RelativePath<E> {
    generation: u64,
    path: SmallVec<[E; 4]>,
}

impl<E> RelativePath<E> {
    fn new(generation: u64) -> Self {
        Self {
            generation,
            path: SmallVec::new(),
        }
    }
}

impl<E: StaticEvent + Clone + Eq> EventReplay for RelativePath<E> {
    type EventType = E;

    fn push(&mut self, event: Self::EventType) {
        self.path.push(event);
    }

    fn is_prefix_of(&self, other: &Self) -> bool {
        other.path.starts_with(&self.path)
    }

    fn extend_with_slice(&mut self, slice: &[Self::EventType]) {
        self.path.extend(slice.iter().cloned());
    }
}

#[derive(Default, Hash, Debug, PartialEq, Eq, PartialOrd, Ord, Clone)]
pub struct ID(u64);

impl ID {
    pub fn new() -> Self {
        static ID_FRONT: AtomicU64 = AtomicU64::new(0);
        Self(ID_FRONT.fetch_add(1, std::sync::atomic::Ordering::AcqRel))
    }
}

// TODO improfve locking scheme in layout and usage

pub struct BFScheduler<T, E, C, R> {
    current_root: Mutex<T>,
    root_generation: AtomicU64,
    task_pool: Mutex<rustc_hash::FxHashMap<ID, (C, RelativePath<E>)>>,
    #[allow(clippy::type_complexity)]
    frontier: Mutex<VecDeque<RelativePath<E>>>,
    _result: PhantomData<R>,
}

impl<T: Default, E, C, R> Default for BFScheduler<T, E, C, R> {
    fn default() -> Self {
        let root = T::default();
        let mut frontier = VecDeque::new();
        frontier.push_back(RelativePath::new(0));
        Self {
            current_root: root.into(),
            root_generation: AtomicU64::new(0),
            task_pool: rustc_hash::FxHashMap::with_hasher(rustc_hash::FxBuildHasher).into(),
            frontier: frontier.into(),
            _result: PhantomData,
        }
    }
}

impl<T: Default, E, C, R> BFScheduler<T, E, C, R> {
    pub fn new() -> Self {
        Self::default()
    }
}

impl<T, E, C, R> StepScheduler<T, C> for BFScheduler<T, E, C, R>
where
    C: Canceable,
    T: EventReplay<EventType = E> + Clone + Hash + Eq,
    E: StaticEvent + Clone + Hash + Eq,
    R: EventInterpretation,
{
    type StateInterpretation = R;
    type ItemMeta = ID;

    fn next(&self, token: C) -> Result<ScheduledStep<T, Self::ItemMeta>, C> {
        // TODO we can recheck generation every now and then and restart if it has advanced
        let mut queue = self.frontier.lock().unwrap();
        let next_id = ID::new();
        let current_gen = self
            .root_generation
            .load(std::sync::atomic::Ordering::Acquire);

        let root = self.current_root.lock().unwrap();
        let mut pool = self.task_pool.lock().unwrap();

        if queue.is_empty() {
            queue.push_back(RelativePath::new(current_gen));
        }

        if let Some(parent_path) = queue.pop_front() {
            let mut variants = T::EventType::VARIANTS.iter().cloned();

            if let Some(first_variant) = variants.next() {
                let mut scheduled_path = parent_path.clone();
                scheduled_path.path.push(first_variant);

                queue.push_back(scheduled_path.clone());
                for variant in variants {
                    let mut path = parent_path.clone();
                    path.path.push(variant);
                    queue.push_back(path);
                }

                let mut root_clone = root.clone();
                root_clone.extend_with_slice(&scheduled_path.path);

                pool.insert(next_id.clone(), (token, scheduled_path));
                return Ok(ScheduledStep::new(root_clone, next_id));
            }
        }
        Err(token)
    }

    fn put_result(
        &self,
        path: ScheduledStep<T, Self::ItemMeta>,
        event_descriptor: Self::StateInterpretation,
    ) {
        let mut frontier = self.frontier.lock().unwrap();
        let mut root = self.current_root.lock().unwrap();
        let mut pool = self.task_pool.lock().unwrap();
        let current_generation = self
            .root_generation
            .load(std::sync::atomic::Ordering::Acquire);

        let Some((_token, our_rel_path)) = pool.remove(path.meta()) else {
            return;
        };

        if event_descriptor.is_dead() {
            drop(root);
            // reap all children
            // Note that it is possible for a child to be correct. Since we do not search for global optimum, this does not matter. Any path to q-minimality is fine.
            // Note further that this potentially correct child could have returned before us due to timing differnences. In this case it could have updated the root and we would now live in its subtree.
            // This allows jumping across local minima in a contrained manner
            // 1. Cancel and remove active workers on this dead subtree

            frontier.retain(|their_rel_path| !our_rel_path.is_prefix_of(their_rel_path));

            let extracted = pool.extract_if(|_id, (_, their_rel_path)| {
                their_rel_path.generation == our_rel_path.generation
                    && our_rel_path.is_prefix_of(their_rel_path)
            });

            for (_id, (token, _their_rel_path)) in extracted {
                token.cancel();
                // no need to mark their path as explored dead end, since it is only reachable via our path / paths in the frontier queue (which we just reaped)
            }
        } else {
            // reap all non-children and set as root
            // no need to extend self.dead here as these dead paths here are unreachable from the new root anyways
            if self
                .root_generation
                .compare_exchange(
                    current_generation,
                    current_generation + 1,
                    std::sync::atomic::Ordering::AcqRel,
                    std::sync::atomic::Ordering::Acquire,
                )
                .is_err()
            {
                return;
            }

            root.extend_with_slice(&our_rel_path.path);
            drop(root);

            // clear out stale (unreachable) entries, I.e. paths that are not part of this subtree.

            // since we do all work here, we can assume that our gewneration was one equal to the previopus root generation.
            // in other words there exist no entries with generation > current_generation. Entries with current_generation + 1 would already be correct if they existed, thus we can focus on entries with current_generation
            pool.retain(|_id, (token, their_rel_path)| {
                if their_rel_path.generation == current_generation + 1 {
                    // unreachable in current model
                    return true;
                }

                if their_rel_path.generation < current_generation
                    || !our_rel_path.is_prefix_of(their_rel_path)
                {
                    token.cancel();
                    false
                } else {
                    // now relative to our_rel_path and in our generation
                    _ = their_rel_path.path.drain(..our_rel_path.path.len());
                    their_rel_path.generation += 1;
                    true
                }
            });
            frontier.clear();
            // frontier.retain(|rel_path| our_rel_path.is_prefix_of(rel_path));
        }
    }

    fn notify_done(&self) {
        self.frontier.lock().unwrap().clear();
        let mut pool = self.task_pool.lock().unwrap();
        for (_, t) in pool.drain() {
            t.0.cancel();
        }
    }
}
