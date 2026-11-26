//! SARE directory service: friends and online status. Plain HTTP; run it
//! behind a TLS reverse proxy (docs/sare-directory.md).
fn main() -> std::io::Result<()> {
    let address = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:7780".into());
    let listener = std::net::TcpListener::bind(&address)?;
    println!("SARE directory listening on {}", listener.local_addr()?);
    sa_net::directory::serve(listener)
}
