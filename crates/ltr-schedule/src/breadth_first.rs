use std::{
    collections::{VecDeque, hash_map::Entry},
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
    explored: Mutex<rustc_hash::FxHashMap<u64, rustc_hash::FxHashMap<RelativePath<E>, Option<R>>>>,
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
            explored: rustc_hash::FxHashMap::with_hasher(rustc_hash::FxBuildHasher).into(),
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
        let mut explored_pool = self.explored.lock().unwrap();
        let explored = explored_pool.entry(current_gen).or_default();

        if queue.is_empty() {
            queue.push_back(RelativePath::new(current_gen));
        }

        while let Some(parent_path) = queue.pop_front() {
            for variant in T::EventType::VARIANTS.iter().cloned() {
                let mut path_clone = parent_path.clone();
                path_clone.path.push(variant);

                let entry = explored.entry(path_clone.clone());
                match entry {
                    Entry::Vacant(e) => {
                        e.insert(None);
                        let mut root_clone = root.clone();
                        root_clone.extend_with_slice(&path_clone.path);
                        _ = pool.insert(next_id.clone(), (token, path_clone.clone()));
                        queue.push_back(path_clone);
                        return Ok(ScheduledStep::new(root_clone, next_id));
                    }
                    Entry::Occupied(e) if e.get().as_ref().is_none_or(|e| !e.is_dead()) => {
                        queue.push_back(path_clone)
                    }
                    _ => {}
                }
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
            let mut explored = self.explored.lock().unwrap();
            // reap all children
            // Note that it is possible for a child to be correct. Since we do not search for global optimum, this does not matter. Any path to q-minimality is fine.
            // Note further that this potentially correct child could have returned before us due to timing differnences. In this case it could have updated the root and we would now live in its subtree.
            // This allows jumping across local minima in a contrained manner
            let extracted = pool.extract_if(|_id, (_, their_rel_path)| {
                their_rel_path.generation == our_rel_path.generation
                    && our_rel_path.is_prefix_of(their_rel_path)
            });

            for (_id, (token, _their_rel_path)) in extracted {
                token.cancel();
                // no need to mark their path as explored dead end, since it is only reachable via our path
            }
            let this_explored_generation = explored.entry(our_rel_path.generation).or_default();
            this_explored_generation.insert(our_rel_path, Some(event_descriptor));
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
            let mut explored = self.explored.lock().unwrap();

            // since we do all work here, we can assume that our gewneration was one equal to the previopus root generation. in other words there exist no entries with generation > current_generation + 1. Entries with current_generation + 1 are already correcy, thus we can focus on entries with current_generation
            pool.retain(|_id, (token, their_rel_path)| {
                if their_rel_path.generation == current_generation + 1 {
                    unreachable!();
                    #[allow(unreachable_code)]
                    return true;
                }

                if their_rel_path.generation < current_generation
                    || !our_rel_path.is_prefix_of(their_rel_path)
                {
                    token.cancel();
                    false
                } else {
                    let old_exploration_state = explored
                        .get_mut(&their_rel_path.generation)
                        .and_then(|map| map.remove(their_rel_path))
                        .flatten();

                    _ = their_rel_path.path.drain(..our_rel_path.path.len());
                    their_rel_path.generation += 1;

                    _ = explored
                        .entry(current_generation + 1)
                        .or_default()
                        .entry(their_rel_path.clone())
                        .or_insert(old_exploration_state);
                    true
                }
            });
            explored.retain(|&generation, _| generation == current_generation + 1);
            frontier.clear();
        }
    }

    fn notify_done(&self) {
        let mut pool = self.task_pool.lock().unwrap();
        for (_, t) in pool.drain() {
            t.0.cancel();
        }
        self.frontier.lock().unwrap().clear();
    }
}
