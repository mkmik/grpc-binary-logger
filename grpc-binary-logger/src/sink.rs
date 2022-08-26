//! gRPC binary log middleware layer writes binary logs into a [`Sink`].
use super::proto::GrpcLogEntry;
use byteorder::{BigEndian, WriteBytesExt};
use prost::Message;
use std::io;
use std::sync::{Arc, Mutex};

/// Receives [`GrpcLogEntry`] entries capturing all gRPC frames from a [`BinaryLoggerLayer`].
pub trait Sink: Clone + Send + Sync {
    /// The type returned in the event of an error.
    type Error;

    /// The sink receives a [`GrpcLogEntry`] message for every gRPC frame captured by a [`BinaryLoggerLayer`].
    /// The sink owns the log entry and is encourage to process the log in the background without blocking the logger layer.
    /// Errors should be handled (e.g. logged) by the sink.
    fn write(&self, data: GrpcLogEntry, error_logger: &impl ErrorLogger<Self::Error>);
}

/// Passed to a Sink to log errors.
pub trait ErrorLogger<E>: Clone + Send + Sync {
    /// Log error
    fn log_error(&self, error: E);
}

/// An error logger that doesn't log anywhere.
#[derive(Clone, Copy, Debug)]
pub struct NopErrorLogger;

impl<E> ErrorLogger<E> for NopErrorLogger {
    fn log_error(&self, _error: E) {}
}

impl<F, E> ErrorLogger<E> for F
where
    F: Fn(E) + Send + Sync + Clone,
{
    fn log_error(&self, error: E) {
        self(error)
    }
}

/// A simple [`Sink`] implementation that prints to stderr.
#[derive(Default, Clone, Copy, Debug)]
pub struct DebugSink;

impl Sink for DebugSink {
    type Error = ();

    fn write(&self, data: GrpcLogEntry, _error_logger: &impl ErrorLogger<Self::Error>) {
        eprintln!("{:?}", data);
    }
}

/// Write binary log entries to a writer using the gRPC binary logging "framing format" (sadly undocumented),
/// compatible with the official gRPC implementation (C/C++/Java/Go) and with the [binlog](https://github.com/mkmik/binlog) CLI tool.
#[derive(Default, Debug)]
pub struct FileSink<W>
where
    W: io::Write + Send,
{
    writer: Arc<Mutex<W>>,
}

impl<W> Clone for FileSink<W>
where
    W: io::Write + Send,
{
    fn clone(&self) -> Self {
        Self {
            writer: Arc::clone(&self.writer),
        }
    }
}

impl<W> FileSink<W>
where
    W: io::Write + Send,
{
    /// Create a new FileSink that writes to a [`std::io::Write`].
    pub fn new(writer: W) -> Self {
        let writer = Arc::new(Mutex::new(writer));
        Self { writer }
    }

    fn write_log_entry(&self, data: &GrpcLogEntry) -> std::io::Result<()> {
        let mut buf = vec![];
        buf.write_u32::<BigEndian>(data.encoded_len() as u32)?;
        data.encode(&mut buf)?;

        let mut writer = self.writer.lock().expect("not poisoned");
        writer.write_all(&buf)?;
        Ok(())
    }
}

impl<W> Sink for FileSink<W>
where
    W: io::Write + Send,
{
    type Error = ();

    fn write(&self, data: GrpcLogEntry, _error_logger: &impl ErrorLogger<Self::Error>) {
        if let Err(e) = self.write_log_entry(&data) {
            eprintln!("error writing binary log: {:?}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt;

    #[derive(Debug)]
    struct DummyError;

    impl fmt::Display for DummyError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "dummy")
        }
    }

    #[derive(Debug, Clone)]
    struct FailingSink;

    impl Sink for FailingSink {
        type Error = DummyError;

        fn write(&self, _data: GrpcLogEntry, error_logger: &impl ErrorLogger<Self::Error>) {
            error_logger.log_error(DummyError);
        }
    }

    #[derive(Debug, Clone)]
    struct TestErrorLogger(Arc<Mutex<Option<DummyError>>>);

    impl TestErrorLogger {
        fn new() -> Self {
            Self(Arc::new(Mutex::new(None)))
        }
    }

    impl ErrorLogger<DummyError> for TestErrorLogger {
        fn log_error(&self, error: DummyError) {
            *self.0.lock().unwrap() = Some(error);
        }
    }

    #[test]
    fn test_sink_error() {
        let error_logger = TestErrorLogger::new();
        let sink = FailingSink;
        assert!(error_logger.0.lock().unwrap().is_none());
        sink.write(GrpcLogEntry::default(), &error_logger);
        assert!(error_logger.0.lock().unwrap().is_some());
    }
}
