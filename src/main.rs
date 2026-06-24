mod cli;
mod config;
mod parser;

use config::Config;
use parser::traverser::{build_ignore_set, collect_rust_files};

fn main() -> anyhow::Result<()> {
    let project_root = env!("CARGO_MANIFEST_DIR");
    let conf = Config::load("mind.toml")?;
    println!(
        "Hello, world! \nConfig: {:?} \nRoot: {}",
        conf, project_root
    );

    let ignore = build_ignore_set(&conf.ignore)?;
    let rust_files = collect_rust_files(project_root, &ignore)?;

    println!(
        "{} Rust files available \nRust files: {:?}",
        rust_files.len(),
        rust_files
    );

    Ok(())
}
