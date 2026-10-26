use clap::Parser;
fn main() {
    let code = match mass::cli::run(mass::cli::Cli::parse()) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("mass: {}", mass::clean(&format!("{e:#}")));
            if e.downcast_ref::<mass::Usage>().is_some() {
                2
            } else {
                1
            }
        }
    };
    std::process::exit(code);
}
