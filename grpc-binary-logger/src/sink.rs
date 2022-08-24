use super::proto::GrpcLogEntry;
use byteorder::{BigEndian, WriteBytesExt};
use prost::Message;
use std::io;
use std::sync::{Arc, Mutex};

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

/// Write binary log entries to a writer using the gRPC binary logging "framing format" (sadly undocumented),
/// compatible with the official gRPC implementation (C/C++/Java/Go) and with the [binlog](https://github.com/mkmik/binlog) CLI tool.
#[derive(Default, Debug)]
pub struct FileSink<W, E = StderrIOErrorLogger>
where
    W: io::Write + Send,
    E: IOErrorLogger,
{
    writer: Arc<Mutex<W>>,
    error_logger: E,
}

impl<W, E> Clone for FileSink<W, E>
where
    W: io::Write + Send,
    E: IOErrorLogger,
{
    fn clone(&self) -> Self {
        Self {
            writer: Arc::clone(&self.writer),
            error_logger: self.error_logger.clone(),
        }
    }
}

impl<W> FileSink<W, StderrIOErrorLogger>
where
    W: io::Write + Send,
{
    /// Create a new FileSink that writes to a [`std::io::Write`].
    pub fn new(writer: W) -> Self {
        let writer = Arc::new(Mutex::new(writer));
        let error_logger = StderrIOErrorLogger;
        Self {
            writer,
            error_logger,
        }
    }
}

impl<W, E> FileSink<W, E>
where
    W: io::Write + Send,
    E: IOErrorLogger,
{
    /// Convert into an existing [`FileSink`] into a [`FileSink`] with a specific error logger.
    pub fn with_error_logger<E2>(self, error_logger: E2) -> FileSink<W, E2>
    where
        E2: IOErrorLogger,
    {
        FileSink {
            writer: self.writer,
            error_logger,
        }
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

impl<W, E> Sink for FileSink<W, E>
where
    W: io::Write + Send,
    E: IOErrorLogger,
{
    fn write(&self, data: GrpcLogEntry) {
        if let Err(e) = self.write_log_entry(&data) {
            eprintln!("error writing binary log: {:?}", e);
        }
    }
}

pub trait IOErrorLogger: Send + Sync + Clone {
    fn log_error(&self, e: std::io::Error);
}

#[derive(Debug, Clone, Copy)]
pub struct StderrIOErrorLogger;

impl IOErrorLogger for StderrIOErrorLogger {
    fn log_error(&self, e: std::io::Error) {
        eprintln!("grpc binlog sink error: {:?}", e);
    }
}

impl<F> IOErrorLogger for F
where
    F: Fn(std::io::Error) + Send + Sync + Clone,
{
    fn log_error(&self, e: std::io::Error) {
        self(e)
    }
}
