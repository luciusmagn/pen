use std::convert::Into;
use std::fmt;
use std::collections::HashMap;
use std::error::Error;
use std::fs::File;
use std::path::PathBuf;
use std::net::ToSocketAddrs;

use hyper;
use hyper::method::Method;
use hyper::status::StatusCode;
use hyper::server::Request as HTTPRequest;
use hyper::server::Response as HTTPResponse;

use types::{
    PencilError,
        PenHTTPError,
        PenUserError,

    UserError,
    PencilResult,
    ViewFunc,
    HTTPErrorHandler,
    UserErrorHandler,
    BeforeRequestFunc,
    AfterRequestFunc,
    TeardownRequestFunc,
};
use wrappers::{
    Request,
    Response,
};
use helpers::{PathBound, send_from_directory_range, redirect};
use serving::run_server;
use routing::{Map, Rule, Matcher};
use http_errors::{HTTPError, NotFound, InternalServerError};
use module::Module;
use typemap::ShareMap;

const DEFAULT_THREADS: usize = 15;

pub struct Pencil {
    pub root_path: String,
    pub name: String,
    pub static_folder: String,
    pub static_url_path: String,
    pub template_folder: String,
    pub extensions: ShareMap,
    pub url_map: Map,
    pub modules: HashMap<String, Module>,
    view_functions: HashMap<String, ViewFunc>,
    before_request_funcs: Vec<Box<BeforeRequestFunc>>,
    after_request_funcs: Vec<Box<AfterRequestFunc>>,
    teardown_request_funcs: Vec<Box<TeardownRequestFunc>>,
    http_error_handlers: HashMap<u16, Box<HTTPErrorHandler>>,
    user_error_handlers: HashMap<String, Box<UserErrorHandler>>,
}

impl Pencil {
    pub fn new(root_path: &str) -> Pencil {
        Pencil {
            root_path: root_path.to_string(),
            name: root_path.to_string(),
            static_folder: String::from("static"),
            static_url_path: String::from("/static"),
            template_folder: String::from("templates"),
            extensions: ShareMap::custom(),
            url_map: Map::new(),
            modules: HashMap::new(),
            view_functions: HashMap::new(),
            before_request_funcs: vec![],
            after_request_funcs: vec![],
            teardown_request_funcs: vec![],
            http_error_handlers: HashMap::new(),
            user_error_handlers: HashMap::new(),
        }
    }

    pub fn is_debug(&self) -> bool { false }
    pub fn is_testing(&self) -> bool { false }

    pub fn route<M: Into<Matcher>, N: AsRef<[Method]>>(&mut self, rule: M, methods: N, endpoint: &str, view_func: ViewFunc) {
        self.add_url_rule(rule.into(), methods.as_ref(), endpoint, view_func);
    }

    pub fn get<M: Into<Matcher>>(&mut self, rule: M, endpoint: &str, view_func: ViewFunc) {
        self.route(rule, &[Method::Get], endpoint, view_func);
    }

    pub fn post<M: Into<Matcher>>(&mut self, rule: M, endpoint: &str, view_func: ViewFunc) {
        self.route(rule, &[Method::Post], endpoint, view_func);
    }

    pub fn delete<M: Into<Matcher>>(&mut self, rule: M, endpoint: &str, view_func: ViewFunc) {
        self.route(rule, &[Method::Delete], endpoint, view_func);
    }

    pub fn patch<M: Into<Matcher>>(&mut self, rule: M, endpoint: &str, view_func: ViewFunc) {
        self.route(rule, &[Method::Patch], endpoint, view_func);
    }

    pub fn put<M: Into<Matcher>>(&mut self, rule: M, endpoint: &str, view_func: ViewFunc) {
        self.route(rule, &[Method::Put], endpoint, view_func);
    }

    pub fn add_url_rule(&mut self, matcher: Matcher, methods: &[Method], endpoint: &str, view_func: ViewFunc) {
        let url_rule = Rule::new(matcher, methods, endpoint);
        self.url_map.add(url_rule);
        self.view_functions.insert(endpoint.to_string(), view_func);
    }

    pub fn register_module(&mut self, module: Module) {
        module.register(self);
    }

    pub fn enable_static_file_handling(&mut self) {
        let rule = self.static_url_path.clone() + "/<filename:path>";
        self.route(&rule as &str, &[Method::Get], "static", send_app_static_file);
    }

    pub fn before_request<F: Fn(&mut Request) -> Option<PencilResult> + Send + Sync + 'static>(&mut self, f: F) {
        self.before_request_funcs.push(Box::new(f));
    }

    pub fn after_request<F: Fn(&Request, &mut Response) + Send + Sync + 'static>(&mut self, f: F) {
        self.after_request_funcs.push(Box::new(f));
    }

    pub fn teardown_request<F: Fn(Option<&PencilError>) + Send + Sync + 'static>(&mut self, f: F) {
        self.teardown_request_funcs.push(Box::new(f));
    }

    pub fn register_http_error_handler<F: Fn(HTTPError) -> PencilResult + Send + Sync + 'static>(&mut self, status_code: u16, f: F) {
        self.http_error_handlers.insert(status_code, Box::new(f));
    }

    pub fn register_user_error_handler<F: Fn(UserError) -> PencilResult + Send + Sync + 'static>(&mut self, error_desc: &str, f: F) {
        self.user_error_handlers.insert(error_desc.to_string(), Box::new(f));
    }

    pub fn httperrorhandler<F: Fn(HTTPError) -> PencilResult + Send + Sync + 'static>(&mut self, status_code: u16, f: F) {
        self.register_http_error_handler(status_code, f);
    }

    pub fn usererrorhandler<F: Fn(UserError) -> PencilResult + Send + Sync + 'static>(&mut self, error_desc: &str, f: F) {
        self.register_user_error_handler(error_desc, f);
    }

    fn preprocess_request(&self, request: &mut Request) -> Option<PencilResult> {
        if let Some(module) = self.get_module(request.module_name()) {
            for func in &module.before_request_funcs {
                if let Some(result) = func(request) {
                    return Some(result);
                }
            }
        }
        for func in &self.before_request_funcs {
            if let Some(result) = func(request) {
                return Some(result);
            }
        }
        None
    }

    fn dispatch_request(&self, request: &mut Request) -> PencilResult {
        if let Some(ref routing_error) = request.routing_error {
            Err(PenHTTPError(routing_error.clone()))
        }
        else if let Some((ref redirect_url, redirect_code)) = request.routing_redirect {
            redirect(redirect_url, redirect_code)
        }
        else if let Some(default_options_response) = self.make_default_options_response(request) {
            Ok(default_options_response)
        }
        else {
            match self.view_functions.get(&request.endpoint().unwrap()) {
                Some(&view_func) => view_func(request),
                None => Err(PenHTTPError(NotFound)),
            }
        }
    }

    fn make_default_options_response(&self, request: &Request) -> Option<Response> {
        if let Some(ref rule) = request.url_rule {
            if rule.provide_automatic_options && request.method() == Method::Options {
                let url_adapter = request.url_adapter();
                let mut response = Response::new_empty();
                response.headers.set(hyper::header::Allow(url_adapter.allowed_methods()));
                Some(response)
            }
            else { None }
        } else { None }
    }

    fn get_module(&self, module_name: Option<String>) -> Option<&Module> {
        if let Some(name) = module_name {
            self.modules.get(&name)
        } else { None }
    }

    fn process_response(&self, request: &Request, response: &mut Response) {
        if let Some(module) = self.get_module(request.module_name()) {
            for func in module.after_request_funcs.iter().rev() {
                func(request, response);
            }
        }
        for func in self.after_request_funcs.iter().rev() {
            func(request, response);
        }
    }

    fn do_teardown_request(&self, request: &Request, e: Option<&PencilError>) {
        if let Some(module) = self.get_module(request.module_name()) {
            for func in module.teardown_request_funcs.iter().rev() {
                func(e);
            }
        }
        for func in self.teardown_request_funcs.iter().rev() {
            func(e);
        }
    }

    fn handle_all_error(&self, request: &Request, e: PencilError) -> PencilResult {
        match e {
            PenHTTPError(e) => self.handle_http_error(request, e),
            PenUserError(e) => self.handle_user_error(request, e),
        }
    }

    fn handle_user_error(&self, request: &Request, e: UserError) -> PencilResult {
        if let Some(module) = self.get_module(request.module_name()) {
            if let Some(handler) = module.user_error_handlers.get(&e.desc) {
                return handler(e);
            }
        }
        if let Some(handler) = self.user_error_handlers.get(&e.desc) {
            handler(e)
        } else { Err(PenUserError(e)) }
    }

    fn handle_http_error(&self, request: &Request, e: HTTPError) -> PencilResult {
        if let Some(module) = self.get_module(request.module_name()) {
            if let Some(handler) = module.http_error_handlers.get(&e.code()) {
                return handler(e);
            }
        }
        if let Some(handler) = self.http_error_handlers.get(&e.code()) {
            handler(e)
        } else { Ok(e.to_response()) }
    }

    fn handle_error(&self, request: &Request, e: &PencilError) -> Response {
        self.log_error(request, e);
        let internal_server_error = InternalServerError;
        if let Ok(response) = self.handle_http_error(request, internal_server_error) {
            response
        } else {
            InternalServerError.to_response()
        }
    }

    fn log_error(&self, request: &Request, e: &PencilError) {
        eprintln!("Error on {} [{}]: {}", request.path(), request.method(), e.description());
    }

    fn full_dispatch_request(&self, request: &mut Request) -> Result<Response, PencilError> {
        let result = match self.preprocess_request(request) {
            Some(result) => result,
            None => self.dispatch_request(request),
        };
        let rv = match result {
            Ok(response) => Ok(response),
            Err(e) => self.handle_all_error(request, e),
        };
        match rv {
            Ok(mut response) => {
                self.process_response(request, &mut response);
                Ok(response)
            },
            Err(e) => Err(e),
        }
    }

    pub fn handle_request(&self, request: &mut Request) -> Response {
        request.match_request();
        match self.full_dispatch_request(request) {
            Ok(response) => {
                self.do_teardown_request(request, None);
                response
            },
            Err(e) => {
                let response = self.handle_error(request, &e);
                self.do_teardown_request(request, Some(&e));
                response
            }
        }
    }

    pub fn run<A: ToSocketAddrs>(self, addr: A) {
        run_server(self, addr, DEFAULT_THREADS);
    }

    pub fn run_threads<A: ToSocketAddrs>(self, addr: A, threads: usize) {
        run_server(self, addr, threads);
    }
}

impl hyper::server::Handler for Pencil {
    fn handle(&self, req: HTTPRequest, mut res: HTTPResponse) {
        match Request::new(self, req) {
            Ok(mut request) => {
                let response = self.handle_request(&mut request);
                response.write(request.method(), res);
            }
            Err(_) => {
                *res.status_mut() = StatusCode::BadRequest;
                if let Ok(w) = res.start() {
                    let _ = w.end();
                }
            }
        };
    }
}

impl PathBound for Pencil {
    fn open_resource(&self, resource: &str) -> File {
        let mut pathbuf = PathBuf::from(&self.root_path);
        pathbuf.push(resource);
        File::open(&pathbuf.as_path()).unwrap()
    }
}

impl fmt::Display for Pencil {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "<Pencil application {}>", self.name)
    }
}

impl fmt::Debug for Pencil {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "<Pencil application {}>", self.name)
    }
}

fn send_app_static_file(request: &mut Request) -> PencilResult {
    let mut static_path = PathBuf::from(&request.app.root_path);
    static_path.push(&request.app.static_folder);
    send_from_directory_range(static_path.to_str().unwrap(), &request.view_args["filename"], false, request.headers().get())
}
