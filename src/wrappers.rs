//! This module implements simple request and response objects.

use std::fmt;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::fs::File;
use std::io::{self, Read, Write, Take};
use std::convert;
use std::cell::RefCell;

use hyper;
use hyper::server::request::Request as HttpRequest;
use hyper::uri::RequestUri::{AbsolutePath, AbsoluteUri, Authority, Star};
use hyper::header::{Headers, ContentLength, ContentType, Cookie, Host};
use hyper::mime::Mime;
use hyper::method::Method;
use hyper::http::h1::HttpReader;
use hyper::net::NetworkStream;
use hyper::buffer::BufReader;
use url::Url;
use url::form_urlencoded;
use formdata::FilePart;
use typemap::TypeMap;

use app::Pencil;
use datastructures::MultiDict;
use httputils::{get_name_by_http_code, get_content_type, get_host_value};
use httputils::get_status_from_code;
use routing::{Rule, MapAdapterMatched, MapAdapter};
use types::ViewArgs;
use http_errors::HTTPError;
use formparser::FormDataParser;
use lazycell::LazyCell;

pub struct Request<'r, 'a, 'b: 'a> {
    pub app: &'r Pencil,
    pub remote_addr: SocketAddr,
    pub method: Method,
    pub headers: Headers,
    pub url: Url,
    pub url_rule: Option<Rule>,
    pub view_args: ViewArgs,
    pub routing_redirect: Option<(String, u16)>,
    pub routing_error: Option<HTTPError>,
    pub extensions_data: TypeMap,
    pub host: Host,
    body: RefCell<HttpReader<&'a mut BufReader<&'b mut NetworkStream>>>,
    args: LazyCell<MultiDict<String>>,
    form: LazyCell<MultiDict<String>>,
    files: LazyCell<MultiDict<FilePart>>,
}

impl<'r, 'a, 'b: 'a> Request<'r, 'a, 'b> {
    pub fn new(app: &'r Pencil, http_request: HttpRequest<'a, 'b>) -> Result<Request<'r, 'a, 'b>, String> {
        let (remote_addr, method, headers, uri, _, body) = http_request.deconstruct();
        let host = match headers.get::<hyper::header::Host>() {
            Some(host) => host.clone(),
            None => return Err("No host specified in your request".into()),
        };
        let url = match uri {
            AbsolutePath(ref path) => {
                let url_string = format!("http://{}{}", get_host_value(&host), path);
                match Url::parse(&url_string) {
                    Ok(url) => url,
                    Err(e) => return Err(format!("Couldn't parse requested URL: {}", e)),
                }
            },
            AbsoluteUri(ref url) => url.clone(),
            Authority(_) | Star => return Err("Unsupported request URI".into()),
        };
        Ok(Request {
            app: app,
            remote_addr: remote_addr,
            method: method,
            headers: headers,
            url: url,
            url_rule: None,
            view_args: HashMap::new(),
            routing_redirect: None,
            routing_error: None,
            extensions_data: TypeMap::new(),
            body: RefCell::new(body),
            host: host,
            args: LazyCell::new(),
            form: LazyCell::new(),
            files: LazyCell::new(),
        })
    }

    pub fn url_adapter(&self) -> MapAdapter {
        self.app.url_map.bind(self.host(), self.path(), self.query_string(), self.method())
    }

    pub fn match_request(&mut self) {
        let url_adapter = self.app.url_map.bind(self.host(), self.path(), self.query_string(), self.method());
        match url_adapter.matched() {
            MapAdapterMatched::MatchedRule((rule, view_args)) => {
                self.url_rule = Some(rule);
                self.view_args = view_args;
            },
            MapAdapterMatched::MatchedRedirect((redirect_url, redirect_code)) => {
                self.routing_redirect = Some((redirect_url, redirect_code));
            },
            MapAdapterMatched::MatchedError(routing_error) => {
                self.routing_error = Some(routing_error);
            },
        }
    }

    pub fn endpoint(&self) -> Option<String> {
        match self.url_rule {
            Some(ref rule) => Some(rule.endpoint.clone()),
            None => None,
        }
    }

    pub fn module_name(&self) -> Option<String> {
        if let Some(endpoint) = self.endpoint() {
            if endpoint.contains('.') {
                let v: Vec<&str> = endpoint.rsplitn(2, '.').collect();
                return Some(v[1].to_string());
            }
        }
        None
    }

    pub fn args(&self) -> &MultiDict<String> {
        if !self.args.filled() {
            let mut args = MultiDict::new();
            if let Some(query) = self.query_string() {
                let pairs = form_urlencoded::parse(query.as_bytes());
                for (k, v) in pairs.into_owned() {
                    args.add(k, v);
                }
            }
            self.args.fill(args).expect("This was checked to be empty!");
        }
        self.args.borrow().expect("This is checked to be always filled")
    }

    fn content_type(&self) -> Option<ContentType> {
        let content_type: Option<&ContentType> = self.headers.get();
        content_type.cloned()
    }

    fn load_form_data(&self) {
        if self.form.filled() { return; }
        let (form, files) = match self.content_type() {
            Some(ContentType(mimetype)) => {
                let parser = FormDataParser::new();
                parser.parse(&mut *self.body.borrow_mut(), &self.headers, &mimetype)
            },
            None => (MultiDict::new(), MultiDict::new()),
        };
        self.form.fill(form).expect("This was checked to be empty!");
        self.files.fill(files).expect("This was checked to be empty!");
    }

    pub fn form(&self) -> &MultiDict<String> {
        self.load_form_data();
        self.form.borrow().expect("This is always checked to be filled.")
    }

    pub fn files(&self) -> &MultiDict<FilePart> {
        self.load_form_data();
        self.files.borrow().expect("This is always checked to be filled.")
    }

    pub fn headers(&self) -> &Headers {
        &self.headers
    }

    pub fn path(&self) -> String {
        self.url.path().to_owned()
    }

    pub fn full_path(&self) -> String {
        let path = self.path();
        let query_string = self.query_string();
        if query_string.is_some() {
            path + "?" + &query_string.unwrap()
        } else { path }
    }

    pub fn host(&self) -> String {
        get_host_value(&self.host)
    }

    pub fn query_string(&self) -> Option<String> {
        self.url.query().map(|q| q.to_owned())
    }

    pub fn cookies(&self) -> Option<&Cookie> {
        self.headers.get()
    }

    pub fn method(&self) -> Method {
        self.method.clone()
    }

    pub fn remote_addr(&self) -> SocketAddr {
        self.remote_addr
    }

    pub fn host_url(&self) -> String {
        "http://".to_owned() + &self.host() + "/"
    }

    pub fn url(&self) -> String {
        self.host_url() + self.full_path().trim_left_matches('/')
    }

    pub fn base_url(&self) -> String {
        self.host_url() + self.path().trim_left_matches('/')
    }
}

impl<'r, 'a, 'b: 'a> fmt::Debug for Request<'r, 'a, 'b> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "<Pencil Request '{}' {}>", self.url(), self.method())
    }
}

impl<'r, 'a, 'b: 'a> Read for Request<'r, 'a, 'b> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.body.borrow_mut().read(buf)
    }
}

pub struct ResponseBody<'a>(Box<Write + 'a>);

impl<'a> ResponseBody<'a> {
    pub fn new<W: Write + 'a>(writer: W) -> ResponseBody<'a> {
        ResponseBody(Box::new(writer))
    }
}

impl<'a> Write for ResponseBody<'a> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

pub trait BodyWrite: Send {
    fn write_body(&mut self, body: &mut ResponseBody) -> io::Result<()>;
}

impl BodyWrite for Vec<u8> {
    fn write_body(&mut self, body: &mut ResponseBody) -> io::Result<()> {
        body.write_all(self)
    }
}

impl<'a> BodyWrite for &'a [u8] {
    fn write_body(&mut self, body: &mut ResponseBody) -> io::Result<()> {
        body.write_all(self)
    }
}

impl BodyWrite for String {
    fn write_body(&mut self, body: &mut ResponseBody) -> io::Result<()> {
        self.as_bytes().write_body(body)
    }
}

impl<'a> BodyWrite for &'a str {
    fn write_body(&mut self, body: &mut ResponseBody) -> io::Result<()> {
        self.as_bytes().write_body(body)
    }
}

impl BodyWrite for File {
    fn write_body(&mut self, body: &mut ResponseBody) -> io::Result<()> {
        io::copy(self, body).map(|_| ())
    }
}

impl BodyWrite for Take<File> {
    fn write_body(&mut self, body: &mut ResponseBody) -> io::Result<()> {
        io::copy(self, body).map(|_| ())
    }
}

pub struct Response {
    pub status_code: u16,
    pub headers: Headers,
    pub body: Option<Box<BodyWrite>>,
}

impl Response {
    pub fn new<T: 'static + BodyWrite>(body: T) -> Response {
        let mut response = Response {
            status_code: 200,
            headers: Headers::new(),
            body: Some(Box::new(body)),
        };
        let mime: Mime = "text/html; charset=UTF-8".parse().unwrap();
        response.headers.set(ContentType(mime));
        response
    }

    pub fn new_empty() -> Response {
        Response {
            status_code: 200,
            headers: Headers::new(),
            body: None,
        }
    }

    pub fn status_name(&self) -> &str {
        match get_name_by_http_code(self.status_code) {
            Some(name) => name,
            None => "UNKNOWN",
        }
    }

    pub fn content_type(&self) -> Option<&ContentType> {
        self.headers.get()
    }

    pub fn set_content_type(&mut self, mimetype: &str) {
        let mimetype = get_content_type(mimetype, "UTF-8");
        let mime: Mime = (&mimetype).parse().unwrap();
        let content_type = ContentType(mime);
        self.headers.set(content_type);
    }

    pub fn content_length(&self) -> Option<usize> {
        let content_length: Option<&ContentLength> = self.headers.get();
        match content_length {
            Some(&ContentLength(length)) => Some(length as usize),
            None => None,
        }
    }

    pub fn set_content_length(&mut self, value: usize) {
        let content_length = ContentLength(value as u64);
        self.headers.set(content_length);
    }

    pub fn set_cookie(&mut self, cookie: hyper::header::SetCookie) {
        self.headers.set(cookie);
    }

    pub fn write(self, request_method: Method, mut res: hyper::server::Response) {
        let status_code = self.status_code;
        *res.status_mut() = get_status_from_code(status_code);
        *res.headers_mut() = self.headers;
        if request_method == Method::Head ||
           (100 <= status_code && status_code < 200) || status_code == 204 || status_code == 304 {
            res.headers_mut().set(ContentLength(0));
            try_return!(res.start().and_then(|w| w.end()));
        } else {
            match self.body {
                Some(mut body) => {
                    let mut res = try_return!(res.start());
                    try_return!(body.write_body(&mut ResponseBody::new(&mut res)));
                    try_return!(res.end());
                },
                None => {
                    res.headers_mut().set(ContentLength(0));
                    try_return!(res.start().and_then(|w| w.end()));
                }
            }
        }
    }
}

impl fmt::Debug for Response {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "<Pencil Response [{}]>", self.status_code)
    }
}

impl convert::From<Vec<u8>> for Response {
    fn from(bytes: Vec<u8>) -> Response {
        let content_length = bytes.len();
        let mut response = Response::new(bytes);
        response.set_content_length(content_length);
        response
    }
}

impl<'a> convert::From<&'a [u8]> for Response {
    fn from(bytes: &'a [u8]) -> Response {
        bytes.to_vec().into()
    }
}

impl<'a> convert::From<&'a str> for Response {
    fn from(s: &'a str) -> Response {
        s.to_owned().into()
    }
}

impl convert::From<String> for Response {
    fn from(s: String) -> Response {
        s.into_bytes().into()
    }
}

impl convert::From<File> for Response {
    fn from(f: File) -> Response {
        let content_length = match f.metadata() {
            Ok(metadata) => Some(metadata.len()),
            Err(_) => None
        };
        let mut response = Response::new(f);
        if let Some(content_length) = content_length {
            response.set_content_length(content_length as usize);
        }
        response
    }
}
