#![allow(unknown_lints,
         new_without_default_derive,
         type_complexity)]

extern crate hyper;
extern crate regex;
extern crate url;
extern crate formdata;
extern crate typemap;
extern crate mime;
extern crate mime_guess;
extern crate lazycell;

/* public api */
pub use app::Pencil;
pub use types::{
    PencilError,
        PenHTTPError,
        PenUserError,
    UserError,
    PencilResult,
    ViewArgs,
    ViewFunc,
    UserErrorHandler,
    HTTPErrorHandler,
    BeforeRequestFunc,
    AfterRequestFunc,
    TeardownRequestFunc,
};
pub use wrappers::{
    Request,
    Response,
};
pub use http_errors::{
    HTTPError
};
pub use helpers::{
    PathBound,
    safe_join,
    abort,
    redirect,
    escape,
    send_file,
    send_from_directory,
};
pub use hyper::header::{Cookie, SetCookie, Headers, ContentLength, ContentType};

#[macro_use] mod utils;
pub mod http_errors;
pub mod datastructures;
pub mod wrappers;
pub mod routing;
pub mod helpers;
pub mod method;
mod app;
mod types;
mod serving;
mod httputils;
mod formparser;
