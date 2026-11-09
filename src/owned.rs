//! Closures that one value owns and that clone by cloning what they capture.
//!
//! A closure that several values need is never shared: each value holds a copy, and a closure
//! that captures only `Copy` or `Clone` data is itself `Clone`.

/// Defines a boxed closure type with a single owner that is `Clone` when the closure it was made
/// from is.
macro_rules! owned_fn {
    (
        $(#[$meta:meta])*
        $vis:vis $name:ident<$($generic:ident),*>, $caller:ident, ($($arg:ident: $ty:ty),*)
    ) => {
        $crate::owned::owned_fn! {
            $(#[$meta])*
            $vis $name<$($generic),*>, $caller, ($($arg: $ty),*) -> ()
        }
    };
    (
        $(#[$meta:meta])*
        $vis:vis $name:ident<$($generic:ident),*>, $caller:ident, ($($arg:ident: $ty:ty),*) -> $ret:ty
    ) => {
        trait $caller<$($generic),*>: Send + Sync {
            fn call(&self, $($arg: $ty),*) -> $ret;
            fn clone_box(&self) -> Box<dyn $caller<$($generic),*>>;
        }

        impl<$($generic,)* F> $caller<$($generic),*> for F
        where
            F: Fn($($ty),*) -> $ret + Clone + Send + Sync + 'static,
        {
            fn call(&self, $($arg: $ty),*) -> $ret {
                self($($arg),*)
            }

            fn clone_box(&self) -> Box<dyn $caller<$($generic),*>> {
                Box::new(self.clone())
            }
        }

        $(#[$meta])*
        $vis struct $name<$($generic),*>(Box<dyn $caller<$($generic),*>>);

        impl<$($generic),*> Clone for $name<$($generic),*> {
            fn clone(&self) -> Self {
                Self(self.0.clone_box())
            }
        }

        impl<$($generic),*> $name<$($generic),*> {
            /// Wraps `function`, which is cloned whenever this value is.
            pub fn new(
                function: impl Fn($($ty),*) -> $ret + Clone + Send + Sync + 'static,
            ) -> Self {
                Self(Box::new(function))
            }

            /// Calls the closure.
            pub fn call(&self, $($arg: $ty),*) -> $ret {
                self.0.call($($arg),*)
            }
        }
    };
}

pub(crate) use owned_fn;
