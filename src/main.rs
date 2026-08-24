use anyhow::Context;
use clap::Parser;
use devmind::config::Config;
use indicatif::{ProgressBar, ProgressStyle};
use devmind::embeddings;
use devmind::parser::{self, 
    traverser::{build_ignore_set, collect_rust_files},
};
use tokio::main;
use devmind::cli::{self, Cli, Commands, ConfigOptions};
use colored::*;

#[main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let conf = Config::load(cli.config.as_deref())?;

    // Initialize configuration for first user before before attempting connection to Ahnlich
    if let Commands::Config { action } = cli.command {
        match action {
            cli::ConfigAction::Init { scope, ahnlich_addr, store, ignore, force } => {
                Config::init(scope, ahnlich_addr, store, ignore, force)?;
            },
            cli::ConfigAction::Get { key } => match key {
                ConfigOptions::AhnlichAddr => println!("ahnlich-addr: {}", conf.ahnlich_addr),
                ConfigOptions::Store => println!("store: {}", conf.store),
                ConfigOptions::Ignore => println!("ignore: {}", conf.ignore.join(", "))
            }
        }

        return Ok(());
    };


    let ahnlich_ai_proxy = embeddings::ahnlich::CodeIndex::new(
        &conf.ahnlich_addr, &conf.store
    ).await.with_context(|| "Failed to connect to Ahnlich AI. Check `ahnlich-addr`".yellow())?;
    
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

            // Reset the peak counter *here*, right before the actual indexing
            // work starts, so connection setup / arg parsing isn't counted
            // toward the number we care about.
            #[cfg(feature = "mem_profile")]
            devmind::memtrack::reset_peak();

            let mut total_chunks = 0usize;
            let mut total_error_chunks = 0usize;

            for file in &rust_files {
                progress_bar.set_message(file.to_owned());
                let chunks = parser::parse_file(file)?;
                
                for chunk in &chunks {
                    if let Err(_e) = ahnlich_ai_proxy.add_chuck(chunk).await {
                        progress_bar.println(format!("{}: {} [{}]   {}", 
                            "Error pushing chunk".yellow(),
                            chunk.item_name.dimmed(),
                            chunk.kind.to_string().dimmed(),
                            chunk.file_path.yellow()
                        ));
                        total_error_chunks += 1;
                        continue;
                    }
                    total_chunks += 1;
                }
                progress_bar.inc(1);
            }

            progress_bar.finish_with_message(format!(
                "Indexed {total_chunks} chunks across {} files. Encountered error while indexing {total_error_chunks}.",
                rust_files.len()
            ));

            #[cfg(feature = "mem_profile")]
            println!(
                "{}",
                format!(
                    "Peak heap allocated during indexing: {}",
                    devmind::memtrack::human_bytes(devmind::memtrack::peak_bytes())
                ).cyan()
            );
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
        Commands::Config { action: _ } => unreachable!(), // Config variant has already been handled above, before Ahnlich connection.
    }
    

    Ok(())
}
