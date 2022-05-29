mod predicate;
pub use self::predicate::{NoReflection, Predicate};
mod sink;
pub use self::sink::Sink;

pub mod proto {
    tonic::include_proto!("grpc.binarylog.v1");
}
pub use proto::GrpcLogEntry;

mod binary_logger;
pub use binary_logger::BinaryLoggerLayer;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        let result = 2 + 2;
        assert_eq!(result, 4);
    }
}
