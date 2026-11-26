//! SARE directory service: friends and online status. Plain HTTP; run it
//! behind a TLS reverse proxy (docs/sare-directory.md).
//!
//! The address is the first argument. Without one it listens on
//! `0.0.0.0:$PORT` when `PORT` is set (hosts such as Render set it), else on
//! `127.0.0.1:7780`.
fn main() -> std::io::Result<()> {
    let address = std::env::args().nth(1).unwrap_or_else(|| {
        match std::env::var("PORT").ok().filter(|p| !p.trim().is_empty()) {
            Some(port) => format!("0.0.0.0:{}", port.trim()),
            None => "127.0.0.1:7780".into(),
        }
    });
    let listener = std::net::TcpListener::bind(&address)?;
    println!("SARE directory listening on {}", listener.local_addr()?);
    sa_net::directory::serve(listener)
}
