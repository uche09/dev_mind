mod cli;
mod config;

use config::Config;

fn main() -> anyhow::Result<()> {
    let conf = Config::load("mind.toml")?;
    println!("Hello, world! {:?}", conf);

    Ok(())
}
