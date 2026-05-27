pub trait EventReplay: Sized {
    type EventType;
    fn extend(&self, event: Self::EventType) -> Self;
    fn is_prefix_of(&self, other: &Self) -> bool;
}

pub trait DynamicEventReplay: EventReplay {
    fn children(&self) -> impl Iterator<Item = Self>;
}

pub trait StaticEvent: Sized + 'static {
    const VARIANTS: &'static [Self];
}

pub trait EventInterpretation {
    fn is_dead(&self) -> bool;
}

impl<T> DynamicEventReplay for T
where
    T: EventReplay,
    T::EventType: StaticEvent + Clone,
{
    fn children(&self) -> impl Iterator<Item = Self> {
        T::EventType::VARIANTS
            .iter()
            .cloned()
            .map(|segment| self.extend(segment))
    }
}
