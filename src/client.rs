use error::{Error, ErrorObject, RequestError};
use futures::{Future, Stream};
use hyper;
//use hyper::client::RequestBuilder;
use hyper::header::{Authorization, Basic, ContentType, Headers};
use serde;
use serde_json as json;
use serde_qs as qs;
use std::str::FromStr;
use tokio_core;


#[cfg(feature = "with-rustls")]
use hyper_rustls;

#[derive(Clone, Default)]
pub struct Params {
    pub stripe_account: Option<String>,
}

#[derive(Clone)]
pub struct Client {
    #[cfg(feature = "with-rustls")]
    client: hyper::client::Client<hyper_rustls::HttpsConnector>,
    #[cfg(feature = "with-openssl")]
    client: hyper::client::Client<C>,
    secret_key: String,
    params: Params,
}

impl Client {
    fn url(path: &str) -> hyper::Uri {
        hyper::Uri::from_str(format!("https://api.stripe.com/v1/{}", &path[1..]).as_str())
            .unwrap()
    }

    #[cfg(feature = "with-rustls")]
    pub fn new<Str: Into<String>>(secret_key: Str) -> Self {
        let core = tokio_core::reactor::Core::new().unwrap();
        let handle = core.handle();
        let https = hyper_rustls::HttpsConnector::new(4, &handle);

        let client = hyper::client::Client::configure()
            .connector(https)
            .build(&handle);
        Client {
            client: client,
            secret_key: secret_key.into(),
            params: Params::default(),
        }
    }

    #[cfg(feature = "with-openssl")]
    pub fn new<Str: Into<String>>(secret_key: Str) -> Self {
        use hyper_openssl::OpensslClient;

        let tls = OpensslClient::new().unwrap();
        let connector = HttpsConnector::new(tls);
        let client = hyper::Client::with_connector(connector);
        Client {
            client: client,
            secret_key: secret_key.into(),
            params: Params::default(),
        }
    }

    /// Clones a new client with different params.
    ///
    /// This is the recommended way to send requests for many different Stripe accounts
    /// or with different Meta, Extra, and Expand params while using the same secret key.
    pub fn with(&self, params: Params) -> Self {
        let mut client = self.clone();
        client.params = params;
        client
    }

    /// Sets a value for the Stripe-Account header
    ///
    /// This is recommended if you are acting as only one Account for the lifetime of the client.
    /// Otherwise, prefer `client.with(Params{stripe_account: "acct_ABC", ..})`.
    pub fn set_stripe_account<Str: Into<String>>(&mut self, account_id: Str) {
        self.params.stripe_account = Some(account_id.into());
    }

    pub fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, Error> {
        let url = Client::url(path);
        let mut request = hyper::Request::new(hyper::Method::Get, url);
        self.set_headers(request.headers_mut());
        self.send(request)
    }

    pub fn post<T: serde::de::DeserializeOwned, P: serde::Serialize>(&self, path: &str, params: P) -> Result<T, Error> {
        let url = Client::url(path);
        let body = qs::to_string(&params)?;
        let mut request = hyper::Request::new(hyper::Method::Post, url);
        self.set_headers(request.headers_mut());
        request.set_body(body);
        self.send(request)
    }

    pub fn post_empty<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, Error> {
        let url = Client::url(path);
        let mut request = hyper::Request::new(hyper::Method::Post, url);
        self.set_headers(request.headers_mut());
        self.send(request)
    }

    pub fn delete<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, Error> {
        let url = Client::url(path);
        let mut request = hyper::Request::new(hyper::Method::Delete, url);
        self.set_headers(request.headers_mut());
        self.send(request)
    }

    fn set_headers(&self, headers: &mut Headers) {
        headers.set(Authorization(Basic {
            username: self.secret_key.clone(),
            password: None,
        }));
        headers.set(ContentType::form_url_encoded());
        if let Some(ref account) = self.params.stripe_account {
            headers.set_raw("Stripe-Account", vec![account.as_bytes().to_vec()]);
        }
    }

    fn send<T: serde::de::DeserializeOwned>(&self, request: hyper::Request) -> Result<T, Error> {
        let response = self.client.request(request).wait()?;
        let status = response.status().as_u16();
        let body = response.body()
            .concat2()
            .wait()?
            .to_vec();
        let body = String::from_utf8_lossy(body.as_slice());

        match status {
            200...299 => {}
            _ => {
                let mut err = json::from_str(&body).unwrap_or_else(|err| {
                    let mut req = ErrorObject { error: RequestError::default() };
                    req.error.message = Some(format!("failed to deserialize error: {}", err));
                    req
                });
                err.error.http_status = status;
                return Err(Error::from(err.error));
            }
        }

        json::from_str(&body).map_err(|err| Error::from(err))
    }
}
