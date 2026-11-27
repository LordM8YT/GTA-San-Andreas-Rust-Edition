//! SARE directory service: friends and online status, and publishing worlds
//! to Discover. Plain HTTP; run it behind a TLS reverse proxy
//! (docs/sare-directory.md).
//!
//! The address is the first argument. Without one it listens on
//! `0.0.0.0:$PORT` when `PORT` is set (hosts such as Render set it), else on
//! `127.0.0.1:7780`.
//!
//! Publishing is on when `SARE_WORLDS_REPO` (`owner/name` of the GitHub
//! repository that stores worlds) and `SARE_WORLDS_TOKEN` are set.
//! `SARE_WORLDS_ADMINS` lists friend codes, separated by commas, that may
//! unpublish any world.
fn main() -> std::io::Result<()> {
    let address = std::env::args().nth(1).unwrap_or_else(|| {
        match std::env::var("PORT").ok().filter(|p| !p.trim().is_empty()) {
            Some(port) => format!("0.0.0.0:{}", port.trim()),
            None => "127.0.0.1:7780".into(),
        }
    });
    let listener = std::net::TcpListener::bind(&address)?;
    println!("SARE directory listening on {}", listener.local_addr()?);
    let var = |name| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
    match (var("SARE_WORLDS_REPO"), var("SARE_WORLDS_TOKEN")) {
        (Some(repo), Some(token)) => {
            let store = sa_net::worlds::GitHub::new(repo.trim(), token.trim());
            println!("Publishing worlds to {}", store.catalog_url());
            let admins = var("SARE_WORLDS_ADMINS").unwrap_or_default();
            let admins = admins.split(',').map(|c| c.trim().to_string());
            let worlds = sa_net::worlds::Worlds::new(Box::new(store), admins);
            sa_net::directory::serve_with_worlds(listener, worlds)
        }
        _ => {
            println!("Publishing is off: SARE_WORLDS_REPO and SARE_WORLDS_TOKEN are not set");
            sa_net::directory::serve(listener)
        }
    }
}
