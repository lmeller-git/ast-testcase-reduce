pub trait Canceable {
    fn cancel(&self);
    fn is_cancelled(&self) -> bool;
}
