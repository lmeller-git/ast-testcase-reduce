use std::{
    collections::{HashMap, HashSet, VecDeque, hash_map::Entry},
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

pub struct BFScheduler<T, E, C, R> {
    current_root: Mutex<T>,
    root_generation: AtomicU64,
    task_pool: Mutex<HashMap<RelativePath<E>, C>>,
    dead: Mutex<HashMap<u64, HashSet<RelativePath<E>>>>,
    _result: PhantomData<R>,
}

impl<T: Default, E, C, R> Default for BFScheduler<T, E, C, R> {
    fn default() -> Self {
        Self {
            current_root: T::default().into(),
            root_generation: AtomicU64::new(0),
            task_pool: HashMap::new().into(),
            dead: HashMap::new().into(),
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
    type ItemMeta = u64;

    fn next(&self, token: C) -> Result<ScheduledStep<T, Self::ItemMeta>, C> {
        // TODO we can recheck generation every now and then and abort if it has advanced
        let mut queue = VecDeque::new();
        let current_gen = self
            .root_generation
            .load(std::sync::atomic::Ordering::Acquire);

        queue.push_back(RelativePath::new(current_gen));

        let mut root_clone = self.current_root.lock().unwrap().clone();
        let mut pool = self.task_pool.lock().unwrap();
        let dead_pool = self.dead.lock().unwrap();
        let dead = dead_pool.get(&current_gen).unwrap();

        while let Some(parent_path) = queue.pop_front() {
            for variant in T::EventType::VARIANTS.iter().cloned() {
                let mut path_clone = parent_path.clone();
                path_clone.path.push(variant);

                if dead.contains(&path_clone) {
                    continue;
                }

                if let Entry::Vacant(e) = pool.entry(path_clone.clone()) {
                    e.insert(token);
                    root_clone.extend_with_slice(&path_clone.path);
                    return Ok(ScheduledStep::new(root_clone, current_gen));
                }
                queue.push_back(path_clone);
            }
        }
        Err(token)
    }

    fn put_result(
        &self,
        path: ScheduledStep<T, Self::ItemMeta>,
        event_descriptor: Self::StateInterpretation,
    ) {
        let current_generation = self
            .root_generation
            .load(std::sync::atomic::Ordering::Acquire);
        if current_generation != *path.meta() {
            return;
        }

        let mut root = self.current_root.lock().unwrap();
        let mut pool = self.task_pool.lock().unwrap();
        todo!()
    }

    fn notify_done(&self) {
        let mut pool = self.task_pool.lock().unwrap();
        for (_, t) in pool.drain() {
            t.cancel();
        }
    }
}
