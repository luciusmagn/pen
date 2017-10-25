//! This module implements the form parsing. It supports url-encoded forms
//! as well as multipart uploads.

use std::io::Read;

use url::form_urlencoded;
use hyper::header::Headers;
use datastructures::MultiDict;
use formdata::{read_formdata, FilePart};
use hyper::mime::{Mime, TopLevel, SubLevel};

/// This type implements parsing of form data for Pen. It can parse
/// multipart and url encoded form data.
pub struct FormDataParser;

impl FormDataParser {
    pub fn new() -> FormDataParser {
        FormDataParser
    }

    pub fn parse<B: Read>(&self, body: &mut B, headers: &Headers, mimetype: &Mime) -> (MultiDict<String>, MultiDict<FilePart>) {
        match *mimetype {
            Mime(TopLevel::Application, SubLevel::WwwFormUrlEncoded, _) => {
                let mut body_vec: Vec<u8> = Vec::new();
                match body.read_to_end(&mut body_vec) {
                    Ok(_) => {
                        let mut form = MultiDict::new();
                        for (k, v) in form_urlencoded::parse(&body_vec).into_owned() {
                            form.add(k, v);
                        }
                        (form, MultiDict::new())
                    },
                    Err(_) => (MultiDict::new(), MultiDict::new())
                }
            },
            Mime(TopLevel::Multipart, SubLevel::FormData, _) => {
                match read_formdata(body, headers) {
                    Ok(form_data) => {
                        let mut form = MultiDict::new();
                        let mut files = MultiDict::new();
                        for (name, value) in form_data.fields {
                            form.add(name, value);
                        }
                        for (name, file) in form_data.files {
                            files.add(name, file);
                        }
                        (form, files)
                    },
                    Err(_) => (MultiDict::new(), MultiDict::new())
                }
            },
            _ => (MultiDict::new(), MultiDict::new())
        }
    }
}
