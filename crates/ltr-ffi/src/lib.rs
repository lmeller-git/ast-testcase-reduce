use std::sync::Weak;

use ltr_core::{EventReplay, ScheduledStep, SelectionPolicy, StaticEvent, sync::Canceable};
use ltr_schedule::{BFScheduler as RawBFScheduler, StepScheduler, TreeNode};
use pyo3::prelude::*;

pub struct PyCancelToken(Py<PyAny>);

impl Canceable for PyCancelToken {
    fn cancel(&self) {
        Python::attach(|py| _ = self.0.bind(py).call_method0("set"));
    }

    fn is_cancelled(&self) -> bool {
        // Python::attach(|py| self.0.bind(py).call_method0("get"))
        false
    }
}

#[pyclass]
pub struct BFScheduler {
    inner:
        RawBFScheduler<DDMinPath, DDMinEvent, PyCancelToken, DDMinEvent, DDMinEventInterpretor, 2>,
}

#[pymethods]
impl BFScheduler {
    #[new]
    fn new() -> Self {
        Self {
            inner: RawBFScheduler::new(),
        }
    }

    fn next(&self, cancel_token: Py<PyAny>) -> PyResult<Option<PyScheduledStep>> {
        Ok(self
            .inner
            .next(PyCancelToken(cancel_token))
            .ok()
            .map(|step| PyScheduledStep(Some(step))))
    }

    fn put_result(&self, mut path: PyRefMut<PyScheduledStep>, event: DDMinEventType) {
        self.inner
            .put_result(path.0.take().unwrap(), DDMinEvent(event));
    }
}

#[pyclass(from_py_object)]
#[derive(Clone, Debug)]
pub struct PyScheduledStep(
    Option<ScheduledStep<DDMinPath, Weak<TreeNode<DDMinEvent, PyCancelToken, DDMinEvent, 2>>>>,
);

#[pymethods]
impl PyScheduledStep {
    pub fn path(&self) -> DDMinPath {
        self.0.as_ref().map(|step| step.path().clone()).unwrap()
    }
}

#[pyclass(from_py_object)]
#[derive(Debug, Default, Clone, PartialEq, Eq, Hash)]
pub struct DDMinPath(Vec<DDMinEvent>);

#[pymethods]
impl DDMinPath {
    pub fn to_list(&self) -> Vec<DDMinEventType> {
        self.0.iter().map(|item| item.0).collect()
    }

    fn __str__(&self) -> String {
        let steps: Vec<String> = self.0.iter().map(|event| format!("{}", event.0)).collect();
        steps.join(" -> ")
    }

    fn __repr__(&self) -> String {
        let bools: Vec<String> = self.0.iter().map(|event| format!("{}", event.0)).collect();

        format!("DDMinPath([{}])", bools.join(", "))
    }
}

impl EventReplay for DDMinPath {
    type EventType = DDMinEvent;

    fn push(&mut self, event: Self::EventType) {
        self.0.push(event);
    }

    fn is_prefix_of(&self, other: &Self) -> bool {
        other.0.starts_with(&self.0)
    }

    fn extend_with_slice(&mut self, slice: &[Self::EventType]) {
        self.0.extend_from_slice(slice);
    }
}

type DDMinEventType = bool;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default, Hash)]
pub struct DDMinEvent(DDMinEventType);

impl From<DDMinEventType> for DDMinEvent {
    fn from(value: DDMinEventType) -> Self {
        Self(value)
    }
}

impl StaticEvent for DDMinEvent {
    const VARIANTS: &'static [Self] = &[Self(true), Self(false)];
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
#[pyclass(from_py_object)]
pub struct DDMinEventInterpretor;

#[pymethods]
impl DDMinEventInterpretor {
    #[new]
    pub fn new() -> Self {
        Self
    }
}

impl SelectionPolicy<DDMinEvent> for DDMinEventInterpretor {
    fn compare(a: &DDMinEvent, b: &DDMinEvent) -> std::cmp::Ordering {
        a.0.cmp(&b.0)
    }

    fn may_reject(s: &DDMinEvent) -> bool {
        !s.0
    }

    fn may_accept(s: &DDMinEvent) -> bool {
        s.0
    }
}

#[pymodule]
fn lib_tr(_py: Python, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<DDMinPath>()?;
    m.add_class::<BFScheduler>()?;
    Ok(())
}
