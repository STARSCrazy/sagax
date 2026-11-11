//! Shared building blocks of Sagax.
//!
//! Deliberately small: only what more than one module needs. Today that is the
//! diagnostic [`Sink`], through which algorithms report what they do without
//! depending on any debug UI or log format.

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
}
