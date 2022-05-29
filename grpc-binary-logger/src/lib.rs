#![deny(rustdoc::broken_intra_doc_links, rust_2018_idioms)]
#![warn(
    missing_copy_implementations,
//    missing_docs,
    clippy::explicit_iter_loop,
    clippy::future_not_send,
    clippy::use_self,
    clippy::clone_on_ref_ptr
)]

mod predicate;
pub use self::predicate::{NoReflection, Predicate};
mod sink;
pub use self::sink::Sink;

mod middleware;
pub use middleware::BinaryLoggerLayer;

pub use grpc_binary_logger_proto as proto;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        let result = 2 + 2;
        assert_eq!(result, 4);
    }
}
