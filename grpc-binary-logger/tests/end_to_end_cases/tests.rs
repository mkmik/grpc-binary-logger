use crate::end_to_end_cases::{server::TestService, test_utils::Fixture};
use grpc_binary_logger::DebugSink;
use grpc_binary_logger_test_proto::TestRequest;

#[tokio::test]
async fn test_server() {
    let fixture = Fixture::new(TestService, DebugSink).await.expect("fixture");

    const BASE: u64 = 1;
    let mut client = fixture.client.clone();
    let res = client
        .test_unary(TestRequest { question: BASE })
        .await
        .expect("no errors");
    assert_eq!(res.into_inner().answer, BASE + 1);
}
