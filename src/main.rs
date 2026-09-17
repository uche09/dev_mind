use anyhow::Context;
use clap::Parser;
use devmind::config::Config;
use devmind::parser::chunk::{ChunkTokenBound, CodeChunk};
use devmind::parser::tokenizer::{ChunkSplitter, HuggingFaceCounter};
use indicatif::{ProgressBar, ProgressStyle};
use devmind::embeddings;
use devmind::parser::{self, 
    traverser::{build_ignore_set, collect_rust_files},
};
use devmind::indexer::index_batches_bounded;
use tokio::main;
use devmind::cli::{self, Cli, Commands, ConfigOptions};
use colored::*;
use std::sync::Arc;
use std::time::{Duration, Instant};

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


    let ahnlich_ai_proxy = Arc::new(
        embeddings::ahnlich::CodeIndex::new(
            &conf.ahnlich_addr, &conf.store
        ).await.with_context(|| "Failed to connect to Ahnlich AI. Check `ahnlich-addr`".yellow())?
    );
    
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
            let index_ops_start = Instant::now();
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
            let mut error_embeddings = vec![];
            let token_counter = HuggingFaceCounter::from_embedded()?;
            let splitter = ChunkSplitter::new(&token_counter, ChunkTokenBound::default());
            let mut file_parsing_duration = Duration::default();
            let mut ahnlich_call_duration_per_file = Duration::default();

            for file in &rust_files {
                progress_bar.set_message(file.to_owned());
                let start_file_parsing = Instant::now();
                let raw_chunks = parser::parse_file(file)?;
                
                let mut bounded_chunks: Vec<CodeChunk> = Vec::new();

                for raw_chunk in raw_chunks {
                    match splitter.split(raw_chunk.clone()) {
                        Ok(sub_chunks) => bounded_chunks.extend(sub_chunks),
                        Err(e) => {
                            error_embeddings.push((
                                format!("{} :: {}", file, raw_chunk.item_name),
                                e,
                            ));

                            // Chunks that produces error from the `split()` method likely from the
                            // the token count implementation is excluded from being embedded and reported.
                        }
                    }
                }

                file_parsing_duration += start_file_parsing.elapsed();
                let start_ahnlich_calls_per_file = Instant::now();
                let results = index_batches_bounded(
                    Arc::clone(&ahnlich_ai_proxy), 
                    bounded_chunks, 8, 1
                ).await;

                for (batch_idx, batch_size, res) in results {
                    if let Err(e) = res {
                        let batch_id = format!(
                            "{} batch-{} with {} chunks",
                            file, batch_idx, batch_size
                        );

                        error_embeddings.push((batch_id, e));
                    }

                    total_chunks += batch_size;
                }
                ahnlich_call_duration_per_file +=  start_ahnlich_calls_per_file.elapsed();
                progress_bar.inc(1);
            }

            for (chunk, e) in &error_embeddings {
                progress_bar.println(format!("Indexing `{}` produced an error: {}", chunk, e));
                
            }
            
            progress_bar.finish_with_message(format!(
                "Indexed {total_chunks} chunks across {} files. Encountered error while indexing {}.\
                \n\nIndexing time: {}ms",
                rust_files.len(), error_embeddings.len(), index_ops_start.elapsed().as_millis()
            ));

            println!("Average Time For:\
                \nFile parsing = {}ms\
                \nAhnlich call per file = {}ms\
                \nAhnlich call per chunk = \"{}ms\" (batch processing)",
                file_parsing_duration.as_millis() / rust_files.len() as u128,
                ahnlich_call_duration_per_file.as_millis() / rust_files.len() as u128,
                ahnlich_call_duration_per_file.as_millis() / total_chunks as u128
            );

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
