use std::fmt;
use std::marker::PhantomData;
use std::rc::Rc;

use crate::codec::Framed;
use crate::http::body::MessageBody;
use crate::http::config::{KeepAlive, ServiceConfig};
use crate::http::error::ResponseError;
use crate::http::h1::{Codec, ExpectHandler, H1Service, UpgradeHandler};
use crate::http::h2::H2Service;
use crate::http::helpers::{Data, DataFactory};
use crate::http::request::Request;
use crate::http::response::Response;
use crate::http::service::HttpService;
use crate::service::{IntoServiceFactory, Service, ServiceFactory};

/// A http service builder
///
/// This type can be used to construct an instance of `http service` through a
/// builder-like pattern.
pub struct HttpServiceBuilder<T, S, X = ExpectHandler, U = UpgradeHandler<T>> {
    keep_alive: KeepAlive,
    client_timeout: u64,
    client_disconnect: u64,
    handshake_timeout: u64,
    expect: X,
    upgrade: Option<U>,
    on_connect: Option<Rc<dyn Fn(&T) -> Box<dyn DataFactory>>>,
    _t: PhantomData<(T, S)>,
}

impl<T, S> HttpServiceBuilder<T, S, ExpectHandler, UpgradeHandler<T>> {
    /// Create instance of `ServiceConfigBuilder`
    pub fn new() -> Self {
        HttpServiceBuilder {
            keep_alive: KeepAlive::Timeout(5),
            client_timeout: 3000,
            client_disconnect: 3000,
            handshake_timeout: 5000,
            expect: ExpectHandler,
            upgrade: None,
            on_connect: None,
            _t: PhantomData,
        }
    }
}

impl<T, S, X, U> HttpServiceBuilder<T, S, X, U>
where
    S: ServiceFactory<Config = (), Request = Request>,
    S::Error: ResponseError + 'static,
    S::InitError: fmt::Debug,
    <S::Service as Service>::Future: 'static,
    X: ServiceFactory<Config = (), Request = Request, Response = Request>,
    X::Error: ResponseError,
    X::InitError: fmt::Debug,
    <X::Service as Service>::Future: 'static,
    U: ServiceFactory<Config = (), Request = (Request, Framed<T, Codec>), Response = ()>,
    U::Error: fmt::Display,
    U::InitError: fmt::Debug,
    <U::Service as Service>::Future: 'static,
{
    /// Set server keep-alive setting.
    ///
    /// By default keep alive is set to a 5 seconds.
    pub fn keep_alive<W: Into<KeepAlive>>(mut self, val: W) -> Self {
        self.keep_alive = val.into();
        self
    }

    /// Set server client timeout in milliseconds for first request.
    ///
    /// Defines a timeout for reading client request header. If a client does not transmit
    /// the entire set headers within this time, the request is terminated with
    /// the 408 (Request Time-out) error.
    ///
    /// To disable timeout set value to 0.
    ///
    /// By default client timeout is set to 3 seconds.
    pub fn client_timeout(mut self, val: u64) -> Self {
        self.client_timeout = val;
        self
    }

    /// Set server connection disconnect timeout in milliseconds.
    ///
    /// Defines a timeout for disconnect connection. If a disconnect procedure does not complete
    /// within this time, the connection get dropped.
    ///
    /// To disable timeout set value to 0.
    ///
    /// By default disconnect timeout is set to 3 seconds.
    pub fn disconnect_timeout(mut self, val: u64) -> Self {
        self.client_disconnect = val;
        self
    }

    /// Set server ssl handshake timeout in milliseconds.
    ///
    /// Defines a timeout for connection ssl handshake negotiation.
    /// To disable timeout set value to 0.
    ///
    /// By default handshake timeout is set to 5 seconds.
    pub fn ssl_handshake_timeout(mut self, val: u64) -> Self {
        self.handshake_timeout = val;
        self
    }

    /// Provide service for `EXPECT: 100-Continue` support.
    ///
    /// Service get called with request that contains `EXPECT` header.
    /// Service must return request in case of success, in that case
    /// request will be forwarded to main service.
    pub fn expect<F, X1>(self, expect: F) -> HttpServiceBuilder<T, S, X1, U>
    where
        F: IntoServiceFactory<X1>,
        X1: ServiceFactory<Config = (), Request = Request, Response = Request>,
        X1::Error: ResponseError,
        X1::InitError: fmt::Debug,
        <X1::Service as Service>::Future: 'static,
    {
        HttpServiceBuilder {
            keep_alive: self.keep_alive,
            client_timeout: self.client_timeout,
            client_disconnect: self.client_disconnect,
            handshake_timeout: self.handshake_timeout,
            expect: expect.into_factory(),
            upgrade: self.upgrade,
            on_connect: self.on_connect,
            _t: PhantomData,
        }
    }

    /// Provide service for custom `Connection: UPGRADE` support.
    ///
    /// If service is provided then normal requests handling get halted
    /// and this service get called with original request and framed object.
    pub fn upgrade<F, U1>(self, upgrade: F) -> HttpServiceBuilder<T, S, X, U1>
    where
        F: IntoServiceFactory<U1>,
        U1: ServiceFactory<
            Config = (),
            Request = (Request, Framed<T, Codec>),
            Response = (),
        >,
        U1::Error: fmt::Display,
        U1::InitError: fmt::Debug,
        <U1::Service as Service>::Future: 'static,
    {
        HttpServiceBuilder {
            keep_alive: self.keep_alive,
            client_timeout: self.client_timeout,
            client_disconnect: self.client_disconnect,
            handshake_timeout: self.handshake_timeout,
            expect: self.expect,
            upgrade: Some(upgrade.into_factory()),
            on_connect: self.on_connect,
            _t: PhantomData,
        }
    }

    /// Set on-connect callback.
    ///
    /// It get called once per connection and result of the call
    /// get stored to the request's extensions.
    pub fn on_connect<F, I>(mut self, f: F) -> Self
    where
        F: Fn(&T) -> I + 'static,
        I: Clone + 'static,
    {
        self.on_connect = Some(Rc::new(move |io| Box::new(Data(f(io)))));
        self
    }

    /// Finish service configuration and create *http service* for HTTP/1 protocol.
    pub fn h1<F, B>(self, service: F) -> H1Service<T, S, B, X, U>
    where
        B: MessageBody,
        F: IntoServiceFactory<S>,
        S::Error: ResponseError,
        S::InitError: fmt::Debug,
        S::Response: Into<Response<B>>,
    {
        let cfg = ServiceConfig::new(
            self.keep_alive,
            self.client_timeout,
            self.client_disconnect,
            self.handshake_timeout,
        );
        H1Service::with_config(cfg, service.into_factory())
            .expect(self.expect)
            .upgrade(self.upgrade)
            .on_connect(self.on_connect)
    }

    /// Finish service configuration and create *http service* for HTTP/2 protocol.
    pub fn h2<F, B>(self, service: F) -> H2Service<T, S, B>
    where
        B: MessageBody + 'static,
        F: IntoServiceFactory<S>,
        S::Error: ResponseError + 'static,
        S::InitError: fmt::Debug,
        S::Response: Into<Response<B>> + 'static,
        <S::Service as Service>::Future: 'static,
    {
        let cfg = ServiceConfig::new(
            self.keep_alive,
            self.client_timeout,
            self.client_disconnect,
            self.handshake_timeout,
        );
        H2Service::with_config(cfg, service.into_factory()).on_connect(self.on_connect)
    }

    /// Finish service configuration and create `HttpService` instance.
    pub fn finish<F, B>(self, service: F) -> HttpService<T, S, B, X, U>
    where
        B: MessageBody + 'static,
        F: IntoServiceFactory<S>,
        S::Error: ResponseError + 'static,
        S::InitError: fmt::Debug,
        S::Response: Into<Response<B>> + 'static,
        <S::Service as Service>::Future: 'static,
    {
        let cfg = ServiceConfig::new(
            self.keep_alive,
            self.client_timeout,
            self.client_disconnect,
            self.handshake_timeout,
        );
        HttpService::with_config(cfg, service.into_factory())
            .expect(self.expect)
            .upgrade(self.upgrade)
            .on_connect(self.on_connect)
    }
}
