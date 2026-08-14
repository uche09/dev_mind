mod cli;
mod embeddings;
mod config;
mod parser;
mod utils;
mod search;

use clap::Parser;
use config::Config;
use indicatif::{ProgressBar, ProgressStyle};
use parser::traverser::{build_ignore_set, collect_rust_files};
use tokio::main;
use cli::{Cli, Commands};
use colored::*;

#[main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let conf = Config::load("dev_mind.toml")?;
    let ahnlich_ai_proxy = embeddings::ahnlich::CodeIndex::new(
        &conf.ahnlich_addr, &conf.store
    ).await?;
    
    match ahnlich_ai_proxy.ping().await {
        Ok(_pung) => println!("{}", "Connected to ahnlich".green()),
        Err(e) => {
            println!("Failed to connect to ahnlich: \n{}", e);
            return Err(e);
        }
    }

    match cli.command {
        Commands::Init => {
            ahnlich_ai_proxy.create_store().await?;
            println!("{}", "created store successfuly".green());
        },
        Commands::Index { path } => {
            let ignore = build_ignore_set(&conf.ignore)?;
            let rust_files = collect_rust_files(&path.to_string_lossy(), &ignore)?;
            let progress_bar = ProgressBar::new(rust_files.len() as u64);

            progress_bar.set_style(
                ProgressStyle::with_template(
                    "{spinner:.cyan} [{bar:30.cyan/blue}] {pos}/{len} files {msg}"
                ).unwrap().progress_chars("=>-"),
            );

            let mut total_chunks = 0usize;

            for file in &rust_files {
                progress_bar.set_message(file.to_owned());
                let chunks = parser::parse_file(file)?;
                
                for chunk in &chunks {
                    if let Err(_e) = ahnlich_ai_proxy.add_chuck(chunk).await {
                        progress_bar.println(format!("{}\n{}", 
                            "Store not found.".red(),
                            "Run Init command first to create store".yellow()
                        ));
                        return Ok(())
                    }
                    total_chunks += 1;
                }
                progress_bar.inc(1);
            }

            progress_bar.finish_with_message(format!(
                "Indexed {total_chunks} chunks across {} files",
                rust_files.len()
            ));
        },
        Commands::Ask { query, n } => {
            let query = query.trim();
            if query.is_empty() {
                println!("Please ask a valid question");
                return Ok(())
            }

            let hits = ahnlich_ai_proxy.ask(query, n).await?;

            if hits.is_empty() {
                println!("{}", "No matches. Try rephrasing, or run `devmind index` first.".yellow());
                return Ok(());
            }
            
            for (i, hit) in hits.iter().enumerate() {
                println!("- #{}    {}", i +1, hit)
            }
        },
    }
    

    Ok(())
}
