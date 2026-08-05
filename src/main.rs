mod cli;
mod embeddings;
mod config;
mod parser;
mod utils;
mod search;

use clap::Parser;
use config::Config;
use parser::traverser::{build_ignore_set, collect_rust_files};
use tokio::main;
use cli::{Cli, Commands};
use colored::*;
// use crate::embeddings::ahnlich::CodeIndex;

#[main]
async fn main() -> anyhow::Result<()> {
    let project_root = env!("CARGO_MANIFEST_DIR");
    let conf = Config::load("dev_mind.toml")?;

    let ignore = build_ignore_set(&conf.ignore)?;
    let rust_files = collect_rust_files(project_root, &ignore)?;
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

    let cli = Cli::parse();


    match cli.command {
        Commands::Init => {
            ahnlich_ai_proxy.create_store().await?;
            println!("{}", "created store successfuly".green());
        },
        Commands::Index { path } => {
            
        },
        _ => {},
    }
    

    // println!(
    //     "{} Rust files available \nRust files: {:?}",
    //     rust_files.len(),
    //     rust_files
    // );

    // let mut chunks = vec![];
    // rust_files
    //     .iter()
    //     .filter_map(|p| parser::parse_file(p).ok())
    //     .for_each(|mut v| chunks.append(&mut v));

    // println!("##### Code Chunks #####\n\n");
    // for chunk in chunks {
    //     ahnlich_ai_proxy.add_chuck(&chunk).await?;
    //     println!("Added chunck:");
    //     println!("{}", chunk)
    // }

    // let res = ahnlich_ai_proxy.ask("How is file traversal handled", 3).await?;


    // println!("##### Matches #####\n\n");
    // for entry in res {
    //     println!("Entry: \n{}\n\n", entry)
    // }

    Ok(())
}

// async fn check_ahnlich_connection(ahlich_ai_proxy: &CodeIndex) -> anyhow::Result<()> {
//     match ahlich_ai_proxy.ping().await {
//         Ok(_pung) => Ok(println!("Connected to ahnlich")),
//         Err(e) => {
//             println!("Failed to connect to ahnlich: \n{}", e);
//             return Err(e);
//         }
//     }
// }