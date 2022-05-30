use bytes::Bytes;
use http::header::AsHeaderName;
use http::uri::Authority;
use http::HeaderMap;
use http_body::Body as HttpBody;
use hyper::Body;
use pin_project::pin_project;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;
use tonic::Code;
use tonic::{body::BoxBody, Status};
use tower::{Layer, Service};

use crate::{proto, NoReflection, Predicate, Sink};

#[derive(Debug, Default, Clone)]
pub struct BinaryLoggerLayer<K, P = NoReflection>
where
    K: Sink + Send + Sync,
{
    sink: Arc<K>,
    predicate: P,
}

impl<K> BinaryLoggerLayer<K, NoReflection>
where
    K: Sink + Send + Sync,
{
    pub fn new(sink: K) -> Self {
        Self {
            sink: Arc::new(sink),
            predicate: Default::default(),
        }
    }
}

impl<S, K, P> Layer<S> for BinaryLoggerLayer<K, P>
where
    P: Predicate + Send,
    K: Sink + Send + Sync + 'static,
{
    type Service = BinaryLoggerMiddleware<S, K, P>;

    fn layer(&self, service: S) -> Self::Service {
        BinaryLoggerMiddleware::new(service, Arc::clone(&self.sink), self.predicate.clone())
    }
}

#[derive(Debug, Clone)]
pub struct BinaryLoggerMiddleware<S, K, P>
where
    K: Sink + Send + Sync,
{
    sink: Arc<K>,
    inner: S,
    predicate: P,
    next_call_id: Arc<Mutex<u64>>,
}

impl<S, K, P> BinaryLoggerMiddleware<S, K, P>
where
    K: Sink + Send + Sync,
    P: Predicate + Send,
{
    fn new(inner: S, sink: Arc<K>, predicate: P) -> Self {
        Self {
            sink,
            inner,
            predicate,
            next_call_id: Default::default(),
        }
    }

    fn next_call_id(&self) -> u64 {
        let mut counter = self.next_call_id.lock().unwrap();
        *counter += 1;
        *counter
    }
}

impl<S, K, P> Service<hyper::Request<Body>> for BinaryLoggerMiddleware<S, K, P>
where
    S: Service<hyper::Request<Body>, Response = hyper::Response<BoxBody>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    K: Sink + Send + Sync + 'static,
    P: Predicate + Send,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = futures::future::BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: hyper::Request<Body>) -> Self::Future {
        // This is necessary because tonic internally uses `tower::buffer::Buffer`.
        // See https://github.com/tower-rs/tower/issues/547#issuecomment-767629149
        // for details on why this is necessary
        let clone = self.inner.clone();
        let mut inner = std::mem::replace(&mut self.inner, clone);

        if !self.predicate.should_log(&request) {
            Box::pin(async move { inner.call(request).await })
        } else {
            let call = CallLogger::new(self.next_call_id(), Arc::clone(&self.sink));
            Box::pin(async move {
                let uri = request.uri();
                call.log(LogEntry::ClientHeaders {
                    method: uri.path(),
                    authority: uri.authority(),
                    headers: request.headers(),
                });

                // wrap our logger around the request stream.
                let request = Self::logged_request(request, call.clone());

                // perform the actual request.
                let response = inner.call(request).await?;

                // wrap our logger around the response stream.
                Ok(Self::logged_response(response, call))
            })
        }
    }
}

impl<S, K, P> BinaryLoggerMiddleware<S, K, P>
where
    K: Sink + Send + Sync + 'static,
{
    fn logged_request(req: hyper::Request<Body>, call: CallLogger<K>) -> hyper::Request<Body> {
        let (req_parts, mut req_body) = req.into_parts();

        // We *must* return a Request<Body> because that's what `inner` requires.
        // `Body` is not a trait though, so we cannot wrap it and passively log as
        // tonic consumes bytes from the client connection. Instead we have to construct
        // a `Body` with one of its public constructors. We can create a body that
        // produces its data as obtained asynchronously from a channel.
        // We spawn a task that reads from the request body and forwards bytes to the
        // inner request. While we're streaming data we determine gRPC message boundaries
        // and log messages.
        let (mut sender, client_body) = hyper::Body::channel();
        tokio::spawn(async move {
            while let Some(buf) = req_body.data().await {
                match buf {
                    Ok(buf) => {
                        // `data` returns an actual gRPC frames, even if it has been sent
                        // over multiple tcp segments and over multiple HTTP/2 DATA frames.

                        // TODO(mkm): figure out why the client produces a zero length chunk here.
                        // Ignoring it seems to be the right thing to do.
                        if buf.len() != 0 {
                            call.log(LogEntry::ClientMessage(&buf));
                        }
                        if sender.send_data(buf).await.is_err() {
                            sender.abort();
                            return;
                        }
                    }
                    Err(_err) => {
                        sender.abort();
                        return;
                    }
                }
            }
            // gRPC doesn't use client trailers, but let's forward them nevertheless for completeness.
            match req_body.trailers().await {
                Ok(Some(trailers)) => {
                    if sender.send_trailers(trailers).await.is_err() {
                        sender.abort();
                    }
                }
                Err(_err) => {
                    sender.abort();
                }
                _ => {}
            };
        });
        hyper::Request::from_parts(req_parts, client_body)
    }

    fn logged_response(
        response: hyper::Response<BoxBody>,
        call: CallLogger<K>,
    ) -> hyper::Response<BoxBody> {
        let (parts, inner) = response.into_parts();
        let body = BoxBody::new(BinaryLoggingBody {
            inner,
            headers: parts.headers.clone(),
            call: call.clone(),
        });
        call.log(LogEntry::ServerHeaders(&parts.headers));
        if body.is_end_stream() {
            // When an grpc call doesn't produce any results (either because the result type
            // is empty or because it returns an error immediately), the BinaryLoggingBody
            // won't be able to log anything. We have to log the event here.
            call.log(LogEntry::ServerTrailers(&HeaderMap::default()))
        }
        hyper::Response::from_parts(parts, body)
    }
}

#[pin_project]
struct BinaryLoggingBody<K>
where
    K: Sink + Send + Sync,
{
    #[pin]
    inner: BoxBody,
    headers: HeaderMap,
    call: CallLogger<K>,
}

impl<K> HttpBody for BinaryLoggingBody<K>
where
    K: Sink + Send + Sync,
{
    type Data = bytes::Bytes;

    type Error = Status;

    fn poll_data(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        let call = self.call.clone();
        let data = self.project().inner.poll_data(cx);
        if let Poll::Ready(Some(Ok(ref body))) = data {
            call.log(LogEntry::ServerMessage(body));
        }
        data
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<http::HeaderMap>, Self::Error>> {
        let call = self.call.clone();
        let trailers = self.project().inner.poll_trailers(cx);
        if let Poll::Ready(Ok(Some(ref headers))) = trailers {
            call.log(LogEntry::ServerTrailers(headers));
        }
        trailers
    }

    fn size_hint(&self) -> http_body::SizeHint {
        self.inner.size_hint()
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }
}

enum LogEntry<'a> {
    ClientHeaders {
        method: &'a str,
        authority: Option<&'a Authority>,
        headers: &'a HeaderMap,
    },
    ClientMessage(&'a Bytes),
    ServerHeaders(&'a HeaderMap),
    ServerMessage(&'a Bytes),
    ServerTrailers(&'a HeaderMap),
}

#[derive(Clone)]
struct CallLogger<K>
where
    K: Sink + Send + Sync,
{
    call_id: u64,
    sequence: Arc<Mutex<u64>>,
    sink: Arc<K>,
}

impl<K> CallLogger<K>
where
    K: Sink + Send + Sync,
{
    fn new(call_id: u64, sink: Arc<K>) -> Self {
        Self {
            call_id,
            sequence: Arc::new(Mutex::new(0)),
            sink,
        }
    }
    fn log(&self, entry: LogEntry<'_>) {
        let sequence_id_within_call = {
            let mut seq = self.sequence.lock().unwrap();
            *seq += 1;
            *seq
        };

        let common_entry = proto::GrpcLogEntry {
            call_id: self.call_id,
            sequence_id_within_call,
            logger: proto::grpc_log_entry::Logger::Server as i32,
            ..Default::default()
        };

        let log_entry = match entry {
            LogEntry::ClientHeaders {
                method,
                authority,
                headers,
            } => {
                let timeout = headers.grpc_timeout().map(Into::into);
                proto::GrpcLogEntry {
                    r#type: proto::grpc_log_entry::EventType::ClientHeader as i32,
                    payload: Some(proto::grpc_log_entry::Payload::ClientHeader(
                        proto::ClientHeader {
                            method_name: method.to_string(),
                            metadata: Some(Self::metadata(headers)),
                            authority: authority
                                .map(|a| a.as_str())
                                .unwrap_or_default()
                                .to_string(),
                            timeout,
                        },
                    )),
                    ..common_entry
                }
            }
            LogEntry::ClientMessage(body) => proto::GrpcLogEntry {
                r#type: proto::grpc_log_entry::EventType::ClientMessage as i32,
                payload: Some(Self::message(body)),
                ..common_entry
            },
            LogEntry::ServerHeaders(headers) => proto::GrpcLogEntry {
                r#type: proto::grpc_log_entry::EventType::ServerHeader as i32,
                payload: Some(proto::grpc_log_entry::Payload::ServerHeader(
                    proto::ServerHeader {
                        metadata: Some(Self::metadata(headers)),
                    },
                )),
                ..common_entry
            },
            LogEntry::ServerMessage(body) => proto::GrpcLogEntry {
                r#type: proto::grpc_log_entry::EventType::ServerMessage as i32,
                payload: Some(Self::message(body)),
                ..common_entry
            },
            LogEntry::ServerTrailers(headers) => proto::GrpcLogEntry {
                r#type: proto::grpc_log_entry::EventType::ServerTrailer as i32,
                payload: Some(proto::grpc_log_entry::Payload::Trailer(proto::Trailer {
                    status_code: headers.grpc_status() as u32,
                    status_message: headers.grpc_message().to_string(),
                    metadata: Some(Self::metadata(headers)),
                    status_details: headers.grpc_status_details(),
                })),
                ..common_entry
            },
        };
        self.sink.write(log_entry);
    }

    fn message(bytes: &Bytes) -> proto::grpc_log_entry::Payload {
        let compressed = bytes[0] == 1;
        if compressed {
            unimplemented!("grpc compressed messages");
        }

        const COMPRESSED_FLAG_FIELD_LEN: usize = 1;
        const MESSAGE_LENGTH_FIELD_LEN: usize = 4;
        let data = bytes
            .clone() // cheap
            .into_iter()
            .skip(COMPRESSED_FLAG_FIELD_LEN + MESSAGE_LENGTH_FIELD_LEN)
            .collect::<Vec<_>>();

        proto::grpc_log_entry::Payload::Message(proto::Message {
            length: data.len() as u32,
            data,
        })
    }

    fn metadata(headers: &HeaderMap) -> proto::Metadata {
        proto::Metadata {
            entry: headers
                .iter()
                .filter(|&(key, _)| !is_reserved_header(key))
                .map(|(key, value)| proto::MetadataEntry {
                    key: key.to_string(),
                    value: value.as_bytes().to_vec(),
                })
                .collect(),
        }
    }
}

/// As defined in [binarylog.proto](https://github.com/grpc/grpc-proto/blob/master/grpc/binlog/v1/binarylog.proto)
fn is_reserved_header<K>(key: K) -> bool
where
    K: AsHeaderName,
{
    let key = key.as_str();
    match key {
        "grpc-trace-bin" => false, // this is the only "grpc-" prefixed header that is not ignored.
        "te" | "content-type" | "content-length" | "content-encoding" | "user-agent" => true,
        _ if key.starts_with("grpc-") => true,
        _ => false,
    }
}

trait GrpcHeaderExt {
    fn grpc_message(&self) -> &str;
    fn grpc_status(&self) -> Code;
    fn grpc_status_details(&self) -> Vec<u8>;
    fn grpc_timeout(&self) -> Option<Duration>;

    fn get_grpc_header<K>(&self, key: K) -> Option<&str>
    where
        K: AsHeaderName;
}

impl GrpcHeaderExt for HeaderMap {
    fn grpc_message(&self) -> &str {
        self.get_grpc_header("grpc-message").unwrap_or_default()
    }

    fn grpc_status(&self) -> Code {
        self.get_grpc_header("grpc-status")
            .map(|s| s.as_bytes())
            .map(Code::from_bytes)
            .unwrap_or(Code::Unknown)
    }

    fn grpc_status_details(&self) -> Vec<u8> {
        self.get_grpc_header("grpc-status-details-bin")
            .and_then(|v| base64::decode(v).ok())
            .unwrap_or_default()
    }

    /// Extract a gRPC timeout from the `grpc-timeout` header.
    /// Returns None if the header is not present or not valid.
    fn grpc_timeout(&self) -> Option<Duration> {
        self.get_grpc_header("grpc-timeout")
            .and_then(parse_grpc_timeout)
    }

    fn get_grpc_header<K>(&self, key: K) -> Option<&str>
    where
        K: AsHeaderName,
    {
        self.get(key).and_then(|s| s.to_str().ok())
    }
}

/// Parse a gRPC "Timeout" format (see [gRPC over HTTP2]).
/// Returns None if it cannot parse the format.
///
/// [gRPC over HTTP2]: https://github.com/grpc/grpc/blob/master/doc/PROTOCOL-HTTP2.md
fn parse_grpc_timeout(header_value: &str) -> Option<Duration> {
    if header_value.is_empty() {
        return None;
    }
    let (digits, unit) = header_value.split_at(header_value.len() - 1);
    let timeout: u64 = digits.parse().ok()?;
    match unit {
        "H" => Some(Duration::from_secs(timeout * 60 * 60)),
        "M" => Some(Duration::from_secs(timeout * 60)),
        "S" => Some(Duration::from_secs(timeout)),
        "m" => Some(Duration::from_millis(timeout)),
        "u" => Some(Duration::from_micros(timeout)),
        "n" => Some(Duration::from_nanos(timeout)),
        _ => None,
    }
}
