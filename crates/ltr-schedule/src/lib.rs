mod adaptive;
mod breadth_first;
mod depth_first;

pub use breadth_first::*;
use ltr_core::ScheduledStep;

pub trait StepScheduler<T, C> {
    type StateInterpretation;
    type ItemMeta;

    fn next(&self, token: C) -> Result<ScheduledStep<T, Self::ItemMeta>, C>;
    fn put_result(&self, path: ScheduledStep<T, Self::ItemMeta>, event: Self::StateInterpretation);
    fn notify_done(&self);
    fn is_cancelled(&self, item: &ScheduledStep<T, Self::ItemMeta>) -> bool;
}
