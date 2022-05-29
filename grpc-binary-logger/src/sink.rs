use super::proto::GrpcLogEntry;

pub trait Sink: Clone + Send + Sync {
    fn write(&self, data: GrpcLogEntry);
}

#[derive(Default, Clone)]
pub struct DebugSink;

impl Sink for DebugSink {
    fn write(&self, data: GrpcLogEntry) {
        println!("{:?}", data);
    }
}
