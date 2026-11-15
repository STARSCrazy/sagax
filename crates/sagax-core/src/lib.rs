//! Shared building blocks of Sagax.
//!
//! Deliberately small: only what more than one module needs. Today that is the
//! diagnostic [`Sink`], through which algorithms report what they do without
//! depending on any debug UI or log format, and the shared places: [`Pos`],
//! [`Cell`] and [`HexLayout`].
//!
//! A trace is always a `Sink` parameter: every operation that decides
//! something has an `op_traced(.., sink)` form, and the plain `op(..)` is
//! `op_traced(.., &mut NoopSink)`. No type stores a sink. A facade that
//! drives a sub-module wraps the sub-module's records with [`MapSink`].

mod cell;
mod hex;

pub use cell::{Cell, Pos};
pub use hex::{HexLayout, axial_step};

/// Receives diagnostic events of an algorithm.
///
/// Algorithms take the sink as a generic parameter. With [`NoopSink`] the
/// compiler removes every event, so diagnostics cost nothing when unused.
pub trait Sink<E> {
    /// `false` if events are thrown away anyway; algorithms may skip building them.
    #[inline]
    fn enabled(&self) -> bool {
        true
    }

    /// Takes one event.
    fn emit(&mut self, event: E);
}

/// A sink that drops every event. The default for normal operation.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoopSink;

impl<E> Sink<E> for NoopSink {
    #[inline(always)]
    fn enabled(&self) -> bool {
        false
    }

    #[inline(always)]
    fn emit(&mut self, _event: E) {}
}

impl<E, S: Sink<E> + ?Sized> Sink<E> for &mut S {
    #[inline]
    fn enabled(&self) -> bool {
        (**self).enabled()
    }

    #[inline]
    fn emit(&mut self, event: E) {
        (**self).emit(event);
    }
}

/// Calls a closure for each event.
pub struct CallbackSink<F>(pub F);

impl<E, F: FnMut(E)> Sink<E> for CallbackSink<F> {
    #[inline]
    fn emit(&mut self, event: E) {
        (self.0)(event);
    }
}

/// Hands each event to `inner` after `map`; `enabled` is the inner one.
///
/// The one adapter between trace types: a facade passes
/// `&mut MapSink::new(sink, Outer::Inner)` to the sub-module it drives, so
/// the sub-module's records arrive wrapped, without an allocation.
///
/// ```
/// use sagax_core::{CallbackSink, MapSink, Sink};
///
/// let mut seen = Vec::new();
/// let mut outer = CallbackSink(|e: (u8, u32)| seen.push(e));
/// let mut inner = MapSink::new(&mut outer, |e: u32| (1, e));
/// inner.emit(7);
/// assert_eq!(seen, [(1, 7)]);
/// ```
pub struct MapSink<'a, S: ?Sized, F> {
    inner: &'a mut S,
    map: F,
}

impl<'a, S: ?Sized, F> MapSink<'a, S, F> {
    /// A sink that maps each event with `map` and hands it to `inner`.
    #[inline]
    pub fn new(inner: &'a mut S, map: F) -> Self {
        Self { inner, map }
    }
}

impl<E, D, S: Sink<D> + ?Sized, F: FnMut(E) -> D> Sink<E> for MapSink<'_, S, F> {
    #[inline]
    fn enabled(&self) -> bool {
        self.inner.enabled()
    }

    #[inline]
    fn emit(&mut self, event: E) {
        self.inner.emit((self.map)(event));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_is_disabled() {
        let sink = NoopSink;
        assert!(!<NoopSink as Sink<u32>>::enabled(&sink));
    }

    #[test]
    fn callback_receives_events() {
        let mut seen = Vec::new();
        {
            let mut sink = CallbackSink(|e: u32| seen.push(e));
            sink.emit(1);
            sink.emit(2);
        }
        assert_eq!(seen, [1, 2]);
    }

    #[test]
    fn map_sink_wraps_and_keeps_enabled() {
        let mut seen = Vec::new();
        {
            let mut outer = CallbackSink(|e: Option<u32>| seen.push(e));
            let mut inner = MapSink::new(&mut outer, Some);
            assert!(Sink::<u32>::enabled(&inner));
            inner.emit(3);
        }
        assert_eq!(seen, [Some(3)]);
        let mut off = NoopSink;
        let inner = MapSink::new(&mut off, |e: u32| e + 1);
        assert!(!Sink::<u32>::enabled(&inner));
    }
}
