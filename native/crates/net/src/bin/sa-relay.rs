fn main() -> std::io::Result<()> {
    let address = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:7778".into())
        .parse()
        .map_err(|_| std::io::Error::other("Usage: sa-relay [listen-IP:port]"))?;
    let relay = sa_net::relay::Relay::start(address)?;
    println!("SA session directory/relay listening on {}", relay.address);
    println!("Prototype TCP service: unencrypted. Use a trusted private network.");
    loop {
        std::thread::park();
    }
}
