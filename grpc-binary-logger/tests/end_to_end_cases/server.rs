use futures::Stream;
use std::pin::Pin;
use tonic::{Request, Response, Status};

use grpc_binary_logger_test_proto::{
    test_server, TestRequest, TestServerStreamResponse, TestUnaryResponse,
};

#[derive(Debug, Clone, Copy)]
pub struct TestService;

type PinnedStream<T> = Pin<Box<dyn Stream<Item = Result<T, tonic::Status>> + Send>>;

#[tonic::async_trait]
impl test_server::Test for TestService {
    async fn test_unary(
        &self,
        request: Request<TestRequest>,
    ) -> Result<Response<TestUnaryResponse>, Status> {
        let request = request.into_inner();
        Ok(tonic::Response::new(TestUnaryResponse {
            answer: request.question + 1,
        }))
    }

    type TestServerStreamStream = PinnedStream<TestServerStreamResponse>;

    async fn test_server_stream(
        &self,
        request: Request<TestRequest>,
    ) -> Result<Response<Self::TestServerStreamStream>, Status> {
        let request = request.into_inner();
        let it = (0..=request.question).map(|answer| Ok(TestServerStreamResponse { answer }));
        Ok(tonic::Response::new(Box::pin(futures::stream::iter(it))))
    }
}
