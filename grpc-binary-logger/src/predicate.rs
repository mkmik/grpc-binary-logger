use http::HeaderMap;
use http_body::Body;

/// A [`Predicate`] allows to filter requests before they get processed by a [`Sink`].
pub trait Predicate: Clone {
    /// If this method returns true, the logger layer will capture gRPC frames for this request
    /// and send them to a [`Sink`].
    fn should_log<B>(&self, req: &hyper::Request<B>) -> bool
    where
        B: Body;
}

#[derive(Default, Clone, Debug)]
pub struct LogAll;

impl Predicate for LogAll {
    fn should_log<B>(&self, _req: &hyper::Request<B>) -> bool
    where
        B: Body,
    {
        true
    }
}

impl<F> Predicate for F
where
    F: Fn(&str, &HeaderMap) -> bool + Clone,
{
    fn should_log<B>(&self, req: &hyper::Request<B>) -> bool
    where
        B: Body,
    {
        let method = req.uri().path();
        let headers = req.headers();
        self(method, headers)
    }
}

/// A [`Predicate`] that filters out all [gRPC server reflection](https://github.com/grpc/grpc/blob/master/doc/server-reflection.md) chatter.
#[derive(Default, Clone, Debug, Copy)]
pub struct NoReflection;

impl Predicate for NoReflection {
    fn should_log<B>(&self, req: &hyper::Request<B>) -> bool
    where
        B: Body,
    {
        let method = req.uri().path();
        !method.starts_with("/grpc.reflection.v1alpha.ServerReflection")
    }
}
