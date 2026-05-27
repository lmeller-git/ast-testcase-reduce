#[derive(Debug, Default, PartialEq, Eq, Clone)]
pub struct ScheduledStep<T> {
    recording: T,
}

impl<T> ScheduledStep<T> {
    pub fn new(v: T) -> Self {
        Self { recording: v }
    }
}
