use futures::FutureExt;
use grpc_binary_logger::{BinaryLoggerLayer, Sink};
use grpc_binary_logger_test_proto::{
    test_client::TestClient,
    test_server::{self, TestServer},
};
use std::net::SocketAddr;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::{Channel, Server};

#[derive(Debug)]
pub struct Fixture {
    pub local_addr: String,
    pub client: TestClient<Channel>,
    shutdown_tx: tokio::sync::oneshot::Sender<()>,
}

impl Fixture {
    /// Start up a grpc server listening on `port`, returning
    /// a fixture with the server and client.
    pub async fn new<T, K>(svc: T, sink: K) -> Result<Self, Box<dyn std::error::Error>>
    where
        T: test_server::Test,
        K: Sink + 'static,
    {
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        let addr: SocketAddr = "127.0.0.1:0".parse()?;
        let listener = tokio::net::TcpListener::bind(addr).await?;
        let local_addr = listener.local_addr()?;
        let local_addr = format!("http://{}", local_addr);

        tokio::spawn(async move {
            Server::builder()
                .layer(BinaryLoggerLayer::new(sink))
                .add_service(TestServer::new(svc))
                .serve_with_incoming_shutdown(
                    TcpListenerStream::new(listener),
                    shutdown_rx.map(drop),
                )
                .await
                .unwrap();
        });

        // Give the test server a few ms to become available
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Construct client and send request, extract response
        let client = TestClient::connect(local_addr.clone())
            .await
            .expect("connect");

        Ok(Self {
            local_addr,
            client,
            shutdown_tx,
        })
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let (tmp_tx, _) = tokio::sync::oneshot::channel();
        let shutdown_tx = std::mem::replace(&mut self.shutdown_tx, tmp_tx);
        if let Err(e) = shutdown_tx.send(()) {
            eprintln!("error shutting down text fixture: {:?}", e);
        }
    }
}
