use http::HeaderMap;
use http_body::Body;

pub trait Predicate: Clone {
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

#[derive(Default, Clone, Debug)]
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
