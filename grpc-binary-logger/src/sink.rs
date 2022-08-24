use super::proto::GrpcLogEntry;

/// Receives [`GrpcLogEntry`] entries capturing all gRPC frames from a [`BinaryLoggerLayer`].
pub trait Sink: Clone + Send + Sync {
    /// The sink receives a [`GrpcLogEntry`] message for every gRPC frame captured by a [`BinaryLoggerLayer`].
    /// The sink owns the log entry and is encourage to process the log in the background without blocking the logger layer.
    /// Errors should be handled (e.g. logged) by the sink.
    fn write(&self, data: GrpcLogEntry);
}

/// A simple [`Sink`] implementation that prints to stderr.
#[derive(Default, Clone, Copy, Debug)]
pub struct DebugSink;

impl Sink for DebugSink {
    fn write(&self, data: GrpcLogEntry) {
        eprintln!("{:?}", data);
    }
}
