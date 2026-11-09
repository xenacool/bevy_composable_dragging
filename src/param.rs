//! Numeric parameters of a state that `vary` can change.

use crate::owned::owned_fn;

owned_fn! {
    /// Reads a number out of a state.
    Getter<S>, GetterCall, (state: &S) -> f32
}

owned_fn! {
    /// Writes a number into a state.
    Setter<S>, SetterCall, (state: &mut S, value: f32)
}

/// A number inside a state of type `S`, addressed by a getter and a setter.
///
/// Rust has no runtime field paths, so a parameter carries closures. The [`param!`](crate::param)
/// macro builds them from a field path.
pub struct Param<S> {
    get: Getter<S>,
    set: Setter<S>,
    scale: f32,
}

impl<S> Clone for Param<S> {
    fn clone(&self) -> Self {
        Self {
            get: self.get.clone(),
            set: self.set.clone(),
            scale: self.scale,
        }
    }
}

impl<S> Param<S> {
    /// Creates a parameter from a getter and a setter.
    pub fn new(
        get: impl Fn(&S) -> f32 + Clone + Send + Sync + 'static,
        set: impl Fn(&mut S, f32) + Clone + Send + Sync + 'static,
    ) -> Self {
        Self {
            get: Getter::new(get),
            set: Setter::new(set),
            scale: 1.0,
        }
    }

    /// Sets the typical magnitude of the parameter, which sizes the solver's first steps.
    /// The default is `1.0`; a parameter in the hundreds (pixels) or the hundredths (fractions of
    /// a turn) converges faster with a matching scale.
    #[must_use]
    pub fn with_scale(mut self, scale: f32) -> Self {
        self.scale = scale;
        self
    }

    /// Reads the parameter from `state`.
    pub fn get(&self, state: &S) -> f32 {
        self.get.call(state)
    }

    /// Writes the parameter into `state`.
    pub fn set(&self, state: &mut S, value: f32) {
        self.set.call(state, value);
    }

    /// The typical magnitude of the parameter.
    #[must_use]
    pub fn scale(&self) -> f32 {
        self.scale
    }
}

/// Builds a [`Param`] from a path to an `f32` field of a state type.
///
/// ```
/// use bevy_composable_dragging::param;
///
/// struct Meeting { start: f32 }
/// struct Schedule { meetings: Vec<Meeting> }
///
/// let second_start = param!(Schedule, meetings[1].start);
/// let mut schedule = Schedule { meetings: vec![Meeting { start: 1.0 }, Meeting { start: 2.0 }] };
/// assert_eq!(second_start.get(&schedule), 2.0);
/// second_start.set(&mut schedule, 5.0);
/// assert_eq!(schedule.meetings[1].start, 5.0);
/// ```
#[macro_export]
macro_rules! param {
    ($state:ty, $($path:tt)+) => {
        $crate::Param::<$state>::new(
            move |state: &$state| state.$($path)+,
            move |state: &mut $state, value: f32| state.$($path)+ = value,
        )
    };
}
