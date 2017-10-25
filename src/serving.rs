use std::net::ToSocketAddrs;
use hyper::server::Server;
use app::Pen;

pub fn run_server<A: ToSocketAddrs>(application: Pen, addr: A, threads: usize) {
    let server = Server::http(addr).unwrap();
    server.handle_threads(application, threads).unwrap();
}
