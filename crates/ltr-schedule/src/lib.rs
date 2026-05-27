mod adaptive;
mod breadth_first;
mod depth_first;

pub use breadth_first::*;

pub trait StepScheduler<T, C, E> {
    fn next(&self, token: C) -> Result<T, C>;
    fn put_result(&self, path: T, event: E);
    fn notify_done(&self);
}
