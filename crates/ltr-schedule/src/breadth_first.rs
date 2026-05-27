use std::{
    collections::{HashMap, HashSet, VecDeque},
    hash::Hash,
    sync::Mutex,
};

use ltr_core::{EventInterpretation, EventReplay, StaticEvent, sync::Canceable};

use crate::StepScheduler;

pub struct BFScheduler<T, C> {
    current_root: Mutex<T>,
    task_pool: Mutex<HashMap<T, C>>,
    dead: Mutex<HashSet<T>>,
}

impl<T: Default, C> Default for BFScheduler<T, C> {
    fn default() -> Self {
        Self {
            current_root: T::default().into(),
            task_pool: HashMap::new().into(),
            dead: HashSet::new().into(),
        }
    }
}

impl<T: Default, C> BFScheduler<T, C> {
    pub fn new() -> Self {
        Self::default()
    }
}

impl<T, C, E> StepScheduler<T, C, E> for BFScheduler<T, C>
where
    C: Canceable,
    T: EventReplay<EventType = E> + Clone + Hash + Eq,
    E: StaticEvent + Clone + EventInterpretation,
{
    fn next(&self, token: C) -> Result<T, C> {
        let mut queue = VecDeque::new();
        queue.push_back(self.current_root.lock().unwrap().clone());

        let mut pool = self.task_pool.lock().unwrap();
        let dead = self.dead.lock().unwrap();

        while let Some(parent_path) = queue.pop_front() {
            for variant in E::VARIANTS {
                let child_path = parent_path.extend(variant.clone());
                if dead.contains(&child_path) {
                    continue;
                }

                if !pool.contains_key(&child_path) {
                    pool.insert(child_path.clone(), token);
                    return Ok(child_path);
                }

                queue.push_back(child_path);
            }
        }
        Err(token)
    }

    fn put_result(&self, path: T, event: E) {
        let mut root = self.current_root.lock().unwrap();

        if !root.is_prefix_of(&path) {
            return;
        }

        let mut pool = self.task_pool.lock().unwrap();
        if let Some(active_task) = pool.remove(&path) {
            active_task.cancel();

            if event.is_dead() {
                drop(root);
                let mut dead = self.dead.lock().unwrap();

                dead.insert(path.clone());

                // reap all children
                // Note that it is possible for a child to be correct. Since we do not search for global optimum, this does not matter. Any path to q-minimality is fine.
                // Note further that this potentially correct child could have returned before us due to timing differnences. In this case it could have updated the root and we would now live in its subtree.
                // This allows jumping across local minima in a contrained manner
                let extracted = pool.extract_if(|k, _v| path.is_prefix_of(k));
                for (child_path, item) in extracted {
                    item.cancel();
                    dead.insert(child_path);
                }
            } else {
                // reap all non-children and set as root
                // no need to extend self.dead here as these dead paths here are unreachable from the new root anyways
                *root = path.clone();
                drop(root);

                // clear out stale (unreachable) entries, I.e. paths that are nbot part of this subtree.
                let mut dead = self.dead.lock().unwrap();
                dead.retain(|k| path.is_prefix_of(k));

                let extracted = pool.extract_if(|k, _v| !path.is_prefix_of(k));
                for (_, item) in extracted {
                    item.cancel();
                }
            }
        }
    }

    fn notify_done(&self) {
        let mut pool = self.task_pool.lock().unwrap();
        for (_, t) in pool.drain() {
            t.cancel();
        }
    }
}
