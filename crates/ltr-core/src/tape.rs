use std::cmp::Ordering;

pub trait EventReplay: Sized {
    type EventType;
    fn push(&mut self, event: Self::EventType);
    fn is_prefix_of(&self, other: &Self) -> bool;
    fn extend_with_slice(&mut self, slice: &[Self::EventType]);
}

pub trait DynamicEventReplay: EventReplay {
    fn children(&self) -> impl Iterator<Item = Self>;
}

pub trait StaticEvent: Sized + 'static {
    const VARIANTS: &'static [Self];
}

impl<T> DynamicEventReplay for T
where
    T: EventReplay + Clone,
    T::EventType: StaticEvent + Clone,
{
    fn children(&self) -> impl Iterator<Item = Self> {
        T::EventType::VARIANTS.iter().cloned().map(|segment| {
            let mut t_clone = self.clone();
            t_clone.push(segment);
            t_clone
        })
    }
}

pub trait SelectionPolicy<S> {
    fn compare(a: &S, b: &S) -> Ordering;
    fn may_reject(s: &S) -> bool;
    fn may_accept(s: &S) -> bool;
}
