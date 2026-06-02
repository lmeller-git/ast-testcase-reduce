#![allow(clippy::type_complexity)]

use std::{
    array, cmp,
    collections::VecDeque,
    hash::Hash,
    iter::once,
    marker::PhantomData,
    ops::ControlFlow,
    sync::{Arc, Mutex, Weak, atomic::AtomicU64},
};

use ltr_core::{EventReplay, ScheduledStep, SelectionPolicy, StaticEvent, sync::Canceable};
use smallvec::SmallVec;

use crate::StepScheduler;

#[derive(Hash, Clone, Default, Debug, PartialEq, Eq)]
pub struct RelativePath<E> {
    generation: u64,
    path: SmallVec<[E; 4]>,
}

impl<E> RelativePath<E> {
    pub fn new(generation: u64) -> Self {
        Self {
            generation,
            path: SmallVec::new(),
        }
    }

    pub fn new_from(generation: u64, path: impl Iterator<Item = E>) -> Self {
        Self {
            generation,
            path: path.collect(),
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

pub enum Advanceable {
    Force,
    May,
}

pub struct TreeNode<E, C, S, const N: usize = 2>
where
    C: Canceable,
{
    children: Mutex<[Option<Arc<TreeNode<E, C, S, N>>>; N]>,
    token: C,
    generation: u64,
    result: Mutex<Option<S>>,
    _phantom: PhantomData<E>,
}

impl<E, C, S, const N: usize> TreeNode<E, C, S, N>
where
    C: Canceable,
{
    pub fn new(token: C, generation: u64) -> Self {
        Self {
            children: Mutex::new(array::from_fn(|_| None)),
            token,
            generation,
            result: Mutex::new(None),
            _phantom: PhantomData,
        }
    }

    pub fn walk_subtree<F, R>(zelf: Arc<Self>, f: &mut F) -> R
    where
        F: FnMut(Arc<Self>) -> ControlFlow<R, Arc<Self>>,
    {
        let mut root_node = zelf;
        loop {
            match f(root_node) {
                ControlFlow::Continue(node) => root_node = node,
                ControlFlow::Break(res) => return res,
            }
        }
    }

    pub fn may_advcance<F>(&self, f: F) -> bool
    where
        F: Fn(&S) -> Advanceable,
    {
        let mut may_advance = true;
        for c in self.children.lock().unwrap().iter() {
            match c {
                None => may_advance = false,
                Some(c) if c.result.lock().unwrap().is_none() => may_advance = false,
                Some(c) if let Some(r) = c.result.lock().unwrap().as_ref() => match f(r) {
                    Advanceable::May => {}
                    Advanceable::Force => return true,
                },
                _ => unreachable!(),
            }
        }

        may_advance
    }
}

impl<E, C, S, const N: usize> Drop for TreeNode<E, C, S, N>
where
    C: Canceable,
{
    fn drop(&mut self) {
        self.token.cancel();
    }
}

pub struct Tree<E, C, S, const N: usize = 2>
where
    C: Canceable,
{
    children: Mutex<[Option<Arc<TreeNode<E, C, S, N>>>; N]>,
}

impl<E, C, S, const N: usize> Tree<E, C, S, N>
where
    C: Canceable,
{
    pub fn may_advcance<F>(&self, f: F) -> bool
    where
        F: Fn(&S) -> Advanceable,
    {
        let mut may_advance = true;
        for c in self.children.lock().unwrap().iter() {
            match c {
                None => may_advance = false,
                Some(c) if c.result.lock().unwrap().is_none() => may_advance = false,
                Some(c) if let Some(r) = c.result.lock().unwrap().as_ref() => match f(r) {
                    Advanceable::May => {}
                    Advanceable::Force => return true,
                },
                _ => unreachable!(),
            }
        }

        may_advance
    }
}

// TODO improve locking scheme in layout and usage

pub struct BFScheduler<T, E, C, S, P, const N: usize = 2>
where
    C: Canceable,
{
    current_root: Mutex<T>,
    root_generation: AtomicU64,

    tasks: Mutex<Tree<E, C, S, N>>,
    frontier: Mutex<VecDeque<(RelativePath<E>, Weak<TreeNode<E, C, S, N>>)>>,

    _result: PhantomData<(S, P)>,
}

impl<T, E, C, S, P, const N: usize> Default for BFScheduler<T, E, C, S, P, N>
where
    C: Canceable,
    T: Default,
{
    fn default() -> Self {
        Self {
            current_root: Mutex::default(),
            root_generation: AtomicU64::new(0),
            tasks: Mutex::new(Tree {
                children: Mutex::new(array::from_fn(|_| None)),
            }),
            frontier: Mutex::new(VecDeque::new()),
            _result: PhantomData,
        }
    }
}

impl<T, E, C, S, P, const N: usize> BFScheduler<T, E, C, S, P, N>
where
    C: Canceable,
    T: Default,
{
    pub fn new() -> Self {
        Self::default()
    }
}
impl<T, E, C, S, P, const N: usize> BFScheduler<T, E, C, S, P, N>
where
    C: Canceable,
    T: EventReplay<EventType = E> + Clone + Hash + Eq,
    E: StaticEvent + Clone + Hash + Eq,
    P: SelectionPolicy<S>,
{
    #[allow(non_upper_case_globals)]
    const _is_valid: () = const {
        assert!(
            N == E::VARIANTS.len(),
            "BFScheduler::N should be equal to BFScheduler::E::VARIANTS.len()"
        )
    };
}

impl<T, E, C, S, P, const N: usize> StepScheduler<T, C> for BFScheduler<T, E, C, S, P, N>
where
    C: Canceable,
    T: EventReplay<EventType = E> + Clone,
    E: StaticEvent + Clone + Eq,
    P: SelectionPolicy<S>,
{
    type StateInterpretation = S;
    type ItemMeta = Weak<TreeNode<E, C, S, N>>;

    fn next(&self, token: C) -> Result<ScheduledStep<T, Self::ItemMeta>, C> {
        // TODO we can recheck generation every now and then and restart if it has advanced
        let mut frontier = self.frontier.lock().unwrap();

        // tasks (root) is empty at the beginning (and may be empty later on too)
        let tasks = self.tasks.lock().unwrap();

        let root_guard = self.current_root.lock().unwrap();
        let current_gen = self
            .root_generation
            .load(std::sync::atomic::Ordering::Acquire);
        let root = root_guard.clone();
        drop(root_guard);

        let mut root_children = tasks.children.lock().unwrap();

        for (variant, child) in T::EventType::VARIANTS
            .iter()
            .cloned()
            .zip(root_children.iter_mut())
        {
            if child.is_some() {
                continue;
            }

            let node = Arc::new(TreeNode::new(token, current_gen + 1));
            let weak = Arc::downgrade(&node);
            *child = Some(node);

            frontier.push_back((
                RelativePath::new_from(current_gen, once(variant.clone())),
                weak.clone(),
            ));

            let mut path_stem = root.clone();
            path_stem.push(variant);
            return Ok(ScheduledStep::new(path_stem, weak));
        }
        // drop(root_children);
        // drop(tasks);

        while let Some((mut rel_path, parent_node)) = frontier.pop_front() {
            let Some(parent_node_strong) = parent_node.upgrade() else {
                continue;
            };

            if parent_node_strong
                .result
                .lock()
                .unwrap()
                .as_ref()
                .is_some_and(P::may_reject)
            {
                continue;
            }

            let mut children = parent_node_strong.children.lock().unwrap();

            for (variant, child) in T::EventType::VARIANTS
                .iter()
                .cloned()
                .zip(children.iter_mut())
            {
                if child.is_some() {
                    continue;
                }

                let current_gen = self
                    .root_generation
                    .load(std::sync::atomic::Ordering::Acquire);

                if parent_node_strong.generation < current_gen {
                    // should be unreachable
                    unreachable!();
                }

                if rel_path.generation < current_gen {
                    if (rel_path.generation + rel_path.path.len() as u64) < current_gen {
                        // too far behind. should be unreachable
                        unreachable!();
                    }

                    rel_path
                        .path
                        .drain(..(current_gen - rel_path.generation) as usize);
                    rel_path.generation = current_gen;
                }

                let node = Arc::new(TreeNode::new(token, parent_node_strong.generation + 1));
                let weak = Arc::downgrade(&node);
                *child = Some(node);

                let mut child_path =
                    RelativePath::new_from(rel_path.generation, rel_path.path.iter().cloned());
                child_path.push(variant.clone());

                let mut path_stem = root;
                path_stem.extend_with_slice(&child_path.path);

                frontier.push_front((rel_path, parent_node));
                frontier.push_back((child_path, weak.clone()));

                return Ok(ScheduledStep::new(path_stem, weak));
            }
        }
        Err(token)
    }

    fn put_result(&self, path: ScheduledStep<T, Self::ItemMeta>, event: Self::StateInterpretation) {
        let mut tasks = self.tasks.lock().unwrap();

        let Some(node) = path.meta().upgrade() else {
            // already cancelled
            return;
        };

        let current_generation = self
            .root_generation
            .load(std::sync::atomic::Ordering::Acquire);

        let item_gen = node.generation;

        *node.result.lock().unwrap() = Some(event);

        if node
            .result
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(P::may_reject)
        {
            // reap all children
            // Note that it is possible for a child to be correct. Since we do not search for global optimum, this does not matter. Any path to q-minimality is fine.

            // retain all branches that are
            // a) not a subtree of us

            // then drop all of our children. This will invalidate Weak references in frontier and tasks and ensure no further exploration. This is simply here because we may not be the last of our siblings to be done. in this case we can already remove our subtree
            node.children
                .lock()
                .unwrap()
                .iter_mut()
                .for_each(|c| *c = None);
        }

        // reap all non-children and set as root if our generation == current_generation
        // We check if all siblings of node are done. We know that nodes siblings are roots chidlren, since our generation is root_gen + 1

        if item_gen == current_generation + 1
            && tasks.may_advcance(|r| {
                if P::may_accept(r) {
                    Advanceable::Force
                } else {
                    Advanceable::May
                }
            })
        {
            // we are now the new root.
            // walk our subtree until we find no new suitable root
            // finally update the tree root to drop all tasks not on our subtree and extend root path

            let find_best = |children: &[Option<Arc<TreeNode<E, C, S, N>>>; N]| -> Option<(E, Arc<TreeNode<E, C, S, N>>)> {
                    let mut best = None;
                    for (variant, child) in E::VARIANTS.iter().zip(children.iter()) {
                        let Some(child) = child else { continue; };
                        let res_lock = child.result.lock().unwrap();
                        let Some(res) = res_lock.as_ref() else { continue; };

                        if P::may_reject(res) {
                            continue;
                        }

                        if P::may_accept(res) {
                            return Some((variant.clone(), child.clone()));
                        }

                        if best.as_ref().is_none_or(|(_, best): &(E, Arc<TreeNode<E, C, S, N>>)| {
                            P::compare(best.result.lock().unwrap().as_ref().unwrap(), res) == cmp::Ordering::Less
                        }) {
                            best = Some((variant.clone(), child.clone()));
                        }
                    }
                    best
                };

            let Some((variant, mut lowest_node)) = find_best(&tasks.children.lock().unwrap())
            else {
                return;
            };

            let mut acc = vec![variant];

            TreeNode::walk_subtree(lowest_node.clone(), &mut |node| {
                if !node.may_advcance(|r| {
                    if P::may_accept(r) {
                        Advanceable::Force
                    } else {
                        Advanceable::May
                    }
                }) {
                    return ControlFlow::Break(());
                }
                if let Some((variant, next_node)) = find_best(&node.children.lock().unwrap()) {
                    acc.push(variant);
                    lowest_node = next_node.clone();
                    ControlFlow::Continue(next_node)
                } else {
                    ControlFlow::Break(())
                }
            });

            let mut root = self.current_root.lock().unwrap();
            if self
                .root_generation
                .compare_exchange(
                    current_generation,
                    lowest_node.generation,
                    std::sync::atomic::Ordering::AcqRel,
                    std::sync::atomic::Ordering::Acquire,
                )
                .is_err()
            {
                // some one else updated root before we could do it
                // there is nothing that needs to be done now, as our subtree is already droped
                return;
            }

            root.extend_with_slice(&acc);

            *tasks = Tree {
                children: Mutex::new(lowest_node.children.lock().unwrap().clone()),
            };
        }
    }

    fn notify_done(&self) {
        self.root_generation
            .store(u64::MAX, std::sync::atomic::Ordering::Release);
        self.frontier.lock().unwrap().clear();
        self.tasks
            .lock()
            .unwrap()
            .children
            .get_mut()
            .unwrap()
            .iter_mut()
            .for_each(|ele| *ele = None);
    }

    fn is_cancelled(&self, item: &ScheduledStep<T, Self::ItemMeta>) -> bool {
        item.meta()
            .upgrade()
            .is_none_or(|item| item.token.is_cancelled())
    }
}
