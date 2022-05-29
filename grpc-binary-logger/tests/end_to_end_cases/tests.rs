use crate::end_to_end_cases::{
    server::TestService,
    test_utils::{Fixture, RecordingSink},
};
use assert_matches::assert_matches;
use grpc_binary_logger_proto::{
    grpc_log_entry::{EventType, Payload},
    ClientHeader,
};
use grpc_binary_logger_test_proto::TestRequest;

#[tokio::test]
async fn test_unary() {
    let sink = RecordingSink::new();
    let fixture = Fixture::new(TestService, sink.clone())
        .await
        .expect("fixture");

    {
        const BASE: u64 = 1;
        let mut client = fixture.client.clone();
        let res = client
            .test_unary(TestRequest { question: BASE })
            .await
            .expect("no errors");
        assert_eq!(res.into_inner().answer, BASE + 1);
    }

    // Figure out how to ensure that the sink is fully flushed!
    let entries = sink.entries();
    println!("Sink: {entries:?}");
    assert_eq!(entries.len(), 5);
    assert_eq!(entries[0].r#type(), EventType::ClientHeader);
    assert_matches!(
        entries[0].payload,
        Some(Payload::ClientHeader(ClientHeader { ref method_name, .. })) => assert_eq!(method_name, "/test.Test/TestUnary")
    );
    assert_eq!(entries[1].r#type(), EventType::ClientMessage);
    println!("entres[2]: {:?}", entries[2]);
    //    assert_eq!(entries[2].r#type(), EventType::ServerMessage);
}
