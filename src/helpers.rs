use std::error::Error;
use std::fs::File;
use std::path::{Path, PathBuf};

use std::io::{Seek, Read};
use std::io::SeekFrom::{End, Start};          
use hyper::header::{Location, ContentType, Range, ContentRange, ContentLength};
use hyper::header::ByteRangeSpec::{FromTo, Last, AllFrom};
use hyper::header::ContentRangeSpec::{Bytes};

use mime_guess::{guess_mime_type, Mime};

use wrappers::Response;
use types::{
    PenHTTPError,
    PenResult,
    UserError,
};
use http_errors::{
    HTTPError,
        NotFound,
};

pub trait PathBound {
    fn open_resource(&self, resource: &str) -> File;
}

pub fn safe_join(directory: &str, filename: &str) -> Option<PathBuf> {
    let directory = Path::new(directory);
    let filename = Path::new(filename);
    match filename.to_str() {
        Some(filename_str) => {
            if filename.is_absolute() | (filename_str == "..") | (filename_str.starts_with("../")) {
                None
            } else {
                Some(directory.join(filename_str))
            }
        },
        None => None,
    }
}

pub fn abort(code: u16) -> PenResult {
    Err(PenHTTPError(HTTPError::new(code)))
}

pub fn redirect(location: &str, code: u16) -> PenResult {
    let mut response = Response::from(format!(
"<!DOCTYPE HTML PUBLIC \"-//W3C//DTD HTML 3.2 Final//EN\">
<title>Redirecting...</title>
<h1>Redirecting...</h1>
<p>You should be redirected automatically to target URL: 
<a href=\"{}\">{}</a>.  If not click the link.
", location, location));
    response.status_code = code;
    response.set_content_type("text/html");
    response.headers.set(Location(location.to_string()));
    Ok(response)
}

pub fn escape(s: &str) -> String {
    s.replace("&", "&amp;").replace("<", "&lt;")
     .replace(">", "&gt;").replace("\"", "&quot;")
}

pub fn send_file(filepath: &str, mimetype: Mime, as_attachment: bool) -> PenResult {
    let filepath = Path::new(filepath);
    if !filepath.is_file() {
        return Err(PenHTTPError(NotFound));
    }
    let file = match File::open(&filepath) {
        Ok(file) => file,
        Err(e) => {
            return Err(UserError::new(format!("couldn't open {}: {}", filepath.display(), e.description())).into());
        }
    };
    let mut response: Response = file.into();
    response.headers.set(ContentType(mimetype));
    if as_attachment {
        match filepath.file_name() {
            Some(file) => {
                match file.to_str() {
                    Some(filename) => {
                        let content_disposition = format!("attachment; filename={}", filename);
                        response.headers.set_raw("Content-Disposition", vec![content_disposition.as_bytes().to_vec()]);
                    },
                    None =>
                        return Err(UserError::new("filename unavailable, required for sending as attachment.").into()),
                }
            },
            None =>
                return Err(UserError::new("filename unavailable, required for sending as attachment.").into()),
        }
    }
    Ok(response)
}

pub fn send_file_range(filepath: &str, mimetype: Mime, as_attachment: bool, range: Option<&Range>)
    -> PenResult
{
    let filepath = Path::new(filepath);
    if !filepath.is_file() {
        return Err(PenHTTPError(NotFound));
    }
    let mut file = match File::open(&filepath) {
        Ok(file) => file,
        Err(e) =>
            return Err(UserError::new(format!("couldn't open {}: {}", filepath.display(), e.description())).into()),
    };

    let len = file.metadata().map_err(|_| PenHTTPError(HTTPError::InternalServerError))?.len();
    let mut response: Response = match range {
        Some(&Range::Bytes(ref vec_ranges)) => {
            if vec_ranges.len() != 1 { return Err(PenHTTPError(HTTPError::NotImplemented)) };
            match vec_ranges[0] {
                FromTo(s, e) => {
                    file.seek(Start(s))
                        .map_err(|_| PenHTTPError(HTTPError::InternalServerError))?;
                    let mut resp = Response::new(file.take(e-s+1));
                    resp.status_code = 206;
                    resp.headers.set(ContentLength(e-s+1));
                    resp.headers.set(ContentRange(
                        Bytes{range: Some((s, e)), instance_length: Some(len)}
                    ));
                    resp
                },
                AllFrom(s) => {
                    file.seek(Start(s))
                        .map_err(|_| PenHTTPError(HTTPError::InternalServerError))?;
                    let mut resp = Response::new(file);
                    resp.status_code = 206;
                    resp.headers.set(ContentLength(len-s));
                    resp.headers.set(ContentRange(
                        Bytes{range: Some((s, len-1)), instance_length: Some(len)}
                    ));
                    resp
                },
                Last(l) => {
                    file.seek(End(-(l as i64)))
                        .map_err(|_| PenHTTPError(HTTPError::InternalServerError))?;
                    let mut resp = Response::new(file);
                    resp.status_code = 206;
                    resp.headers.set(ContentLength(l));
                    resp.headers.set(ContentRange(
                        Bytes{range: Some((len-l, len-1)), instance_length: Some(len)}
                    ));
                    resp
                },
            }
        },
        Some(_) => return Err(PenHTTPError(HTTPError::NotImplemented)),
        None => {
            let mut resp = Response::new(file);
            resp.headers.set(ContentLength(len));
            resp
        },
    };

    response.headers.set(ContentType(mimetype));
    if as_attachment {
        match filepath.file_name() {
            Some(file) => {
                match file.to_str() {
                    Some(filename) => {
                        let content_disposition = format!("attachment; filename={}", filename);
                        response.headers.set_raw("Content-Disposition", vec![content_disposition.as_bytes().to_vec()]);
                    },
                    None =>
                        return Err(UserError::new("filename unavailable, required for sending as attachment.").into()),
                }
            },
            None =>
                return Err(UserError::new("filename unavailable, required for sending as attachment.").into()),
        }
    }
    Ok(response)
}

pub fn send_from_directory(directory: &str, filename: &str,
                           as_attachment: bool) -> PenResult {
    match safe_join(directory, filename) {
        Some(filepath) => {
            let mimetype = guess_mime_type(filepath.as_path());
            match filepath.as_path().to_str() {
                Some(filepath) => send_file(filepath, mimetype, as_attachment),
                None => Err(PenHTTPError(NotFound)),
            }
        },
        None =>  Err(PenHTTPError(NotFound)),
    }
}

pub fn send_from_directory_range(directory: &str, filename: &str,
                           as_attachment: bool, range: Option<&Range>)
    -> PenResult
{
    match safe_join(directory, filename) {
        Some(filepath) => {
            let mimetype = guess_mime_type(filepath.as_path());
            match filepath.as_path().to_str() {
                Some(filepath) => {
                    send_file_range(filepath, mimetype, as_attachment, range)
                },
                None => {
                    Err(PenHTTPError(NotFound))
                }
            }
        },
        None => {
            Err(PenHTTPError(NotFound))
        }
    }
}
