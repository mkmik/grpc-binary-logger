use std::io::Cursor;

use crate::end_to_end_cases::{
    server::TestService,
    test_utils::{Fixture, RecordingSink},
};
use assert_matches::assert_matches;
use grpc_binary_logger_proto::{
    grpc_log_entry::{EventType, Payload},
    ClientHeader, Message, Metadata, MetadataEntry, ServerHeader, Trailer,
};
use grpc_binary_logger_test_proto::{TestRequest, TestUnaryResponse};
use prost::Message as _;
use tonic::metadata::MetadataValue;

#[tokio::test]
async fn test_unary() {
    let sink = RecordingSink::new();
    let fixture = Fixture::new(TestService, sink.clone())
        .await
        .expect("fixture");

    const BASE: u64 = 1;
    {
        let mut client = fixture.client.clone();
        let mut req = tonic::Request::new(TestRequest { question: BASE });
        req.metadata_mut().insert(
            "my-client-header",
            MetadataValue::from_static("my-client-header-value"),
        );
        let res = client.test_unary(req).await.expect("no errors");
        assert_eq!(res.into_inner().answer, BASE + 1);
    }

    // Figure out how to ensure that the sink is fully flushed!
    let entries = sink.entries();
    assert_eq!(entries.len(), 5);

    assert_eq!(entries[0].r#type(), EventType::ClientHeader);
    assert_matches!(
        entries[0].payload,
        Some(Payload::ClientHeader(ClientHeader {
            ref method_name,
            metadata: Some(Metadata{ref entry}), ..
        })) if method_name == "/test.Test/TestUnary" => {
            assert_matches!(entry[..], [
                MetadataEntry{ref key, ref value, ..},
            ] if key == "my-client-header" && value == b"my-client-header-value");
        }
    );

    assert_eq!(entries[1].r#type(), EventType::ClientMessage);
    assert_matches!(
        entries[1].payload,
        Some(Payload::Message(Message{length, ref data})) if data.len() == length as usize => {
            let message = TestRequest::decode(Cursor::new(data)).unwrap();
            assert_eq!(message.question, BASE);
        }
    );

    assert_eq!(entries[2].r#type(), EventType::ServerHeader);
    assert_matches!(
        entries[2].payload,
        Some(Payload::ServerHeader(ServerHeader {
            metadata: Some(Metadata{ref entry}), ..
        })) => {
            assert_matches!(entry[..], [
                MetadataEntry{ref key, ref value, ..},
            ] if key == "my-server-header" && value == b"my-server-header-value");
        }
    );

    assert_eq!(entries[3].r#type(), EventType::ServerMessage);
    assert_matches!(
        entries[3].payload,
        Some(Payload::Message(Message{length, ref data})) if data.len() == length as usize => {
            let message = TestUnaryResponse::decode(Cursor::new(data)).unwrap();
            assert_eq!(message.answer, BASE+1);
        }
    );

    assert_eq!(entries[4].r#type(), EventType::ServerTrailer);
    assert_matches!(entries[4].payload, Some(Payload::Trailer(Trailer { .. })));
}
