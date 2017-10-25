# Pen (A Sharp Pen fork (A Pen fork))

[![Build Status](https://travis-ci.org/luciusmagn/pen.svg?branch=master)](https://travis-ci.org/luciusmagn/pen) [![Crates.io Version](https://img.shields.io/crates/v/pen.svg)](https://crates.io/crates/pen/) [![Crates.io LICENSE](https://img.shields.io/crates/l/pen.svg)](https://crates.io/crates/pen/)

A microframework for Rust inspired by Flask.

```rust
extern crate pen;
use pen::{Pen, Request, Response, PenResult};
fn hello(_: &mut Request) -> PenResult {
    Ok(Response::from("Hello World!"))
}
fn main() {
    let mut app = Pen::new("/web/hello");
    app.get("/", "hello", hello);
    app.run("127.0.0.1:5000");
}
```
