use anyhow::Context;
use clap::Parser;
use colored::*;
use devmind::cli::{self, Cli, Commands, ConfigOptions};
use devmind::config::Config;
use devmind::indexer::{group_by_token_budget, index_batches_bounded};
use devmind::parser::chunk::{ChunkTokenBound, CodeChunk};
use devmind::parser::tokenizer::{ChunkSplitter, HuggingFaceCounter};
use devmind::parser::{
    self,
    traverser::{build_ignore_set, collect_rust_files},
};
use devmind::{embeddings, indexer};
use indicatif::{ProgressBar, ProgressStyle};
use std::sync::Arc;
use tokio::main;

#[main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let conf = Config::load(cli.config.as_deref())?;

    // Initialize configuration for first user before before attempting connection to Ahnlich
    if let Commands::Config { action } = cli.command {
        match action {
            cli::ConfigAction::Init {
                scope,
                ahnlich_addr,
                store,
                ignore,
                force,
            } => {
                Config::init(scope, ahnlich_addr, store, ignore, force)?;
            }
            cli::ConfigAction::Get { key } => match key {
                ConfigOptions::AhnlichAddr => println!("ahnlich-addr: {}", conf.ahnlich_addr),
                ConfigOptions::Store => println!("store: {}", conf.store),
                ConfigOptions::Ignore => println!("ignore: {}", conf.ignore.join(", ")),
            },
        }

        return Ok(());
    };

    let ahnlich_ai_proxy = Arc::new(
        embeddings::ahnlich::CodeIndex::new(&conf.ahnlich_addr, &conf.store)
            .await
            .with_context(|| "Failed to connect to Ahnlich AI. Check `ahnlich-addr`".yellow())?,
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
        }
        Commands::Index { path } => {
            let ignore = build_ignore_set(&conf.ignore)?;
            let rust_files = collect_rust_files(&path.to_string_lossy(), &ignore)?;
            let progress_bar = ProgressBar::new(rust_files.len() as u64);

            progress_bar.set_style(
                ProgressStyle::with_template(
                    "{spinner:.cyan} [{bar:30.cyan/blue}] {pos}/{len} files {msg}",
                )
                .unwrap()
                .progress_chars("=>-"),
            );

            
            let mut total_chunks = 0usize;
            let mut error_embeddings = vec![];
            let token_counter = HuggingFaceCounter::from_embedded()?;
            let splitter = ChunkSplitter::new(&token_counter, ChunkTokenBound::default());

            for file in &rust_files {
                progress_bar.set_message(file.to_owned());
                let raw_chunks = parser::parse_file(file)?;

                let mut bounded_chunks: Vec<CodeChunk> = Vec::new();

                for raw_chunk in raw_chunks {
                    match splitter.split(raw_chunk.clone()) {
                        Ok(sub_chunks) => bounded_chunks.extend(sub_chunks),
                        Err(e) => {
                            error_embeddings
                                .push((format!("{} :: {}", file, raw_chunk.item_name), e));

                            // Chunks that produces error from the `split()` method likely from the
                            // the token count implementation is excluded from being embedded and reported.
                        }
                    }
                }

                let batches_by_token_budget = group_by_token_budget(
                    bounded_chunks,
                    indexer::TOEKN_THRESHOLD_FOR_SINGLE_PER_BATCH,
                    4,
                )?;
                let results = index_batches_bounded(
                    Arc::clone(&ahnlich_ai_proxy),
                    batches_by_token_budget,
                    1,
                )
                .await;

                for (batch_idx, batch_size, res) in results {
                    if let Err(e) = res {
                        let batch_id =
                            format!("{} batch-{} with {} chunks", file, batch_idx, batch_size);

                        error_embeddings.push((batch_id, e));
                    }

                    total_chunks += batch_size;
                }
                progress_bar.inc(1);
            }

            for (chunk, e) in &error_embeddings {
                progress_bar.println(format!("Indexing `{}` produced an error: {}", chunk, e));
            }

            progress_bar.finish_with_message(format!(
                "\nIndexed {total_chunks} chunks across {} files. Encountered error while indexing {} batch(es).",
                rust_files.len(), error_embeddings.len()
            ));
            
        }
        Commands::Ask { query, n } => {
            let query = query.trim();
            if query.is_empty() {
                println!("Please ask a valid question");
                return Ok(());
            }

            let hits = ahnlich_ai_proxy.ask(query, n).await?;

            if hits.is_empty() {
                println!(
                    "{}",
                    "No matches. Try rephrasing, or run `devmind index` first.".yellow()
                );
                return Ok(());
            }

            for (i, hit) in hits.iter().enumerate() {
                println!("- #{}    {}", i + 1, hit)
            }
        }
        Commands::Config { action: _ } => unreachable!(), // Config variant has already been handled above, before Ahnlich connection.
    }

    Ok(())
}
