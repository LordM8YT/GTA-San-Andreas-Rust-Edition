fn main() {
    let path = std::env::args_os()
        .nth(1)
        .expect("check-install ORIGINAL_GAME_FOLDER");
    let result = sa_client::install::validate(std::path::Path::new(&path));
    for error in &result.errors {
        eprintln!("{error}");
    }
    for warning in &result.warnings {
        eprintln!("Warning: {warning}");
    }
    println!("Installation ready: {}", result.ready());
    if !result.ready() {
        std::process::exit(1);
    }
}
