use std::collections::HashMap;
use std::mem;
use std::path::PathBuf;

use hyper::method::Method;
use http_errors::NotFound;
use app::Pencil;
use routing::Matcher;
use types::ViewFunc;
use types::{PencilResult, PencilError, HTTPError, UserError};
use types::{BeforeRequestFunc, AfterRequestFunc, TeardownRequestFunc};
use types::{HTTPErrorHandler, UserErrorHandler};
use helpers::send_from_directory_range;
use wrappers::{Request, Response};

pub struct Module {
    pub name: String,
    pub root_path: String,
    pub static_folder: Option<String>,
    pub static_url_path: Option<String>,
    pub template_folder: Option<String>,
    pub before_request_funcs: Vec<Box<BeforeRequestFunc>>,
    pub after_request_funcs: Vec<Box<AfterRequestFunc>>,
    pub teardown_request_funcs: Vec<Box<TeardownRequestFunc>>,
    pub http_error_handlers: HashMap<u16, Box<HTTPErrorHandler>>,
    pub user_error_handlers: HashMap<String, Box<UserErrorHandler>>,
    deferred_functions: Vec<Box<Fn(&mut Pencil) + Send + Sync>>,
    deferred_routes: Vec<(Matcher, Vec<Method>, String, ViewFunc)>,
}

use std::fmt;

impl fmt::Debug for Module {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        write!(f, "Module {{ name: {}, root_path: {}, static_folder: {:?}, static_url_path: {:?}, template_folder: {:?} }}",
            self.name, self.root_path, self.static_folder, self.static_url_path, self.template_folder)
    }
}

impl Module {
    pub fn new(name: &str, root_path: &str) -> Module {
        Module {
            name: name.to_string(),
            root_path: root_path.to_string(),
            static_folder: None,
            static_url_path: None,
            template_folder: None,
            before_request_funcs: Vec::new(),
            after_request_funcs: Vec::new(),
            teardown_request_funcs: Vec::new(),
            http_error_handlers: HashMap::new(),
            user_error_handlers: HashMap::new(),
            deferred_functions: Vec::new(),
            deferred_routes: Vec::new(),
        }
    }

    fn record<F: Fn(&mut Pencil) + Send + Sync + 'static>(&mut self, f: F) {
        self.deferred_functions.push(Box::new(f));
    }

    pub fn route<M: Into<Matcher>, N: AsRef<[Method]>>(&mut self, rule: M, methods: N, endpoint: &str, view_func: ViewFunc) {
        let mut methods_vec: Vec<Method> = Vec::new();
        let endpoint = format!("{}.{}", self.name, endpoint);

        methods_vec.extend(methods.as_ref().iter().cloned());
        if endpoint.contains('.') {
            panic!("Module endpoint should not contain dot");
        }
        self.deferred_routes.push((rule.into(), methods_vec, endpoint, view_func));
    }

    pub fn before_request<F: Fn(&mut Request) -> Option<PencilResult> + Send + Sync + 'static>(&mut self, f: F) {
        self.before_request_funcs.push(Box::new(f));
    }

    pub fn before_app_request<F: Fn(&mut Request) -> Option<PencilResult> + Send + Sync + Clone + 'static>(&mut self, f: F) {
        self.record(move |app| app.before_request(f.clone()));
    }

    pub fn after_request<F: Fn(&Request, &mut Response) + Send + Sync + 'static>(&mut self, f: F) {
        self.after_request_funcs.push(Box::new(f));
    }

    pub fn after_app_request<F: Fn(&Request, &mut Response) + Send + Sync + Clone + 'static>(&mut self, f: F) {
        self.record(move |app| app.after_request(f.clone()));
    }

    pub fn teardown_request<F: Fn(Option<&PencilError>) + Send + Sync + 'static>(&mut self, f: F) {
        self.teardown_request_funcs.push(Box::new(f));
    }

    pub fn teardown_app_request<F: Fn(Option<&PencilError>) + Send + Sync + Clone + 'static>(&mut self, f: F) {
        self.record(move |app| app.teardown_request(f.clone()));
    }

    pub fn httperrorhandler<F: Fn(HTTPError) -> PencilResult + Send + Sync + 'static>(&mut self, status_code: u16, f: F) {
        self.http_error_handlers.insert(status_code, Box::new(f));
    }

    pub fn usererrorhandler<F: Fn(UserError) -> PencilResult + Send + Sync + 'static>(&mut self, error_desc: &str, f: F) {
        self.user_error_handlers.insert(error_desc.to_string(), Box::new(f));
    }

    pub fn app_httperrorhandler<F: Fn(HTTPError) -> PencilResult + Send + Sync + Clone + 'static>(&mut self, status_code: u16, f: F) {
        self.record(move |app| app.httperrorhandler(status_code, f.clone()));
    }

    pub fn app_usererrorhandler<F: Fn(UserError) -> PencilResult + Send + Sync + Clone + 'static>(&mut self, error_desc: &str, f: F) {
        let desc = error_desc.to_string();
        self.record(move |app| app.register_user_error_handler(&desc, f.clone()));
    }

    pub fn register(mut self, app: &mut Pencil) {
        if app.modules.contains_key(&self.name) {
            panic!("A module that is named {} already exists, name collision occurred.", self.name);
        }

        let static_url_path = match self.static_folder {
            Some(_) => {
                match self.static_url_path {
                    Some(ref static_url_path) => Some(static_url_path.clone()),
                    None => None,
                }
            },
            None => None
        };
        if let Some(static_url_path) = static_url_path {
            let mut rule = static_url_path.clone();
            rule += "/<filename:path>";
            self.route(rule, &[Method::Get], "static", send_module_static_file);
        }
        let deferred_routes = mem::replace(&mut self.deferred_routes, Vec::new());
        for (matcher, methods, endpoint, view_func) in deferred_routes {
            app.add_url_rule(matcher, methods.as_ref(), &endpoint, view_func);
        }
        let deferred_functions = mem::replace(&mut self.deferred_functions, Vec::new());
        for deferred in deferred_functions {
            deferred(app);
        }
        app.modules.insert(self.name.clone(), self);
    }
}

fn send_module_static_file(request: &mut Request) -> PencilResult {
    if let Some(module_name) = request.module_name() {
        if let Some(module) = request.app.modules.get(&module_name) {
            if let Some(ref module_static_folder) = module.static_folder {
                let mut static_path = PathBuf::from(&module.root_path);
                static_path.push(module_static_folder);
                return send_from_directory_range(static_path.to_str().unwrap(), &request.view_args["filename"], false, request.headers().get());
            }
        }
    }
    Err(NotFound.into())
}
