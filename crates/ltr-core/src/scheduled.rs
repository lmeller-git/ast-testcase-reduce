#[derive(Debug, Default, PartialEq, Eq, Clone)]
pub struct ScheduledStep<T, M> {
    recording: T,
    meta: M,
}

impl<T, M> ScheduledStep<T, M> {
    pub fn new(recording: T, meta: M) -> Self {
        Self { recording, meta }
    }

    pub fn meta(&self) -> &M {
        &self.meta
    }

    pub fn path(&self) -> &T {
        &self.recording
    }
}
