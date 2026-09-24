use super::metadata::Metadata;
use crate::{parser::chunk::CodeChunk, search::SimNHit, utils::helper};
use ahnlich_client_rs::ai::AiClient;
use ahnlich_types::algorithm::algorithms::Algorithm;
use ahnlich_types::keyval::{
    AiStoreEntry, StoreInput, StoreValue, store_input::Value as AiStoreValue,
};
use ahnlich_types::{
    ai::{
        models::AiModel,
        preprocess::PreprocessAction,
        query::{CreateStore, GetSimN, Set},
        server::{self, Pong},
    },
    metadata::{MetadataValue, metadata_value::Value as MetaValue},
};
use std::collections::HashMap;

/// Ahnlich AI client
pub struct CodeIndex {
    client: AiClient,
    store: String,
}

impl CodeIndex {
    pub async fn new(addr: &str, store: &str) -> anyhow::Result<Self> {
        let client = AiClient::new(addr.to_string()).await?;

        Ok(Self {
            client,
            store: store.to_string(),
        })
    }

    /// Test database connection, returns `Ok(Pong)` if connected
    pub async fn ping(&self) -> anyhow::Result<Pong> {
        Ok(self.client.ping(None).await?)
    }

    /// create store on the running ahnlich db via AI proxy.
    ///
    /// Uses the `JinaEmbeddingsV2BaseCode` model for both storage
    /// and query embeddings.
    pub async fn create_store(&self) -> anyhow::Result<()> {
        self.client
            .create_store(
                CreateStore {
                    store: self.store.to_owned(),
                    query_model: AiModel::JinaEmbeddingsV2BaseCode as i32,
                    index_model: AiModel::JinaEmbeddingsV2BaseCode as i32,
                    predicates: vec![], // metadata key for indexing
                    non_linear_indices: vec![],
                    error_if_exists: false,
                    store_original: false,
                },
                None,
            )
            .await?;
        Ok(())
    }

    pub async fn drop_store(&self) -> anyhow::Result<()> {
        self.client
            .drop_store(
                ahnlich_types::ai::query::DropStore {
                    store: self.store.clone(),
                    error_if_not_exists: false,
                },
                None,
            )
            .await?;

        Ok(())
    }

    /// Generate and store embeddings via Ahnlich Ai proxy
    pub async fn add_chuck(&self, chunk: &CodeChunk) -> anyhow::Result<()> {
        let data_to_store = Set {
            store: self.store.clone(),
            inputs: vec![build_entry(chunk)],
            preprocess_action: PreprocessAction::NoPreprocessing as i32,
            execution_provider: None,
            model_params: HashMap::new(),
        };

        self.client.set(data_to_store, None).await?;

        Ok(())
    }

    /// Generate and store embeddings for many chunks in a single Ahnlich
    /// `Set` call.
    ///
    /// Ahnlich's `Set.inputs` field is already a `Vec<AiStoreEntry>`, one
    /// call can carry many entries. Sending them together lets the AI
    /// proxy run one inference pass across the whole group instead of
    /// paying per-call overhead (gRPC round trip, tokenization setup) once
    /// per chunk. This is the change that should have the biggest effect
    /// on indexing time.
    pub async fn add_chunks_batch(&self, chunks: &[CodeChunk]) -> anyhow::Result<()> {
        let inputs = chunks.iter().map(build_entry).collect();

        let data_to_store = Set {
            store: self.store.clone(),
            inputs,
            preprocess_action: PreprocessAction::NoPreprocessing as i32,
            execution_provider: None,
            model_params: HashMap::new(),
        };

        self.client.set(data_to_store, None).await?;

        Ok(())
    }

    /// Embeds query and perform similarity search against stored vectors via Ahnlich Ai proxy.
    pub async fn ask(&self, query: &str, n: usize) -> anyhow::Result<Vec<SimNHit>> {
        let res: server::GetSimN = self
            .client
            .get_sim_n(
                GetSimN {
                    store: self.store.clone(),
                    search_input: Some(StoreInput {
                        value: Some(AiStoreValue::RawString(query.to_string())),
                    }),
                    closest_n: n as u64,
                    algorithm: Algorithm::CosineSimilarity as i32,
                    execution_provider: None,
                    preprocess_action: PreprocessAction::NoPreprocessing as i32,
                    condition: None,
                    model_params: HashMap::new(),
                },
                None,
            )
            .await?;
        Ok(format_results(res))
    }
}

/// Builds a single Ahnlich store entry (embedding input + metadata) from a
/// `CodeChunk`. So both the single-item and batched paths build entries identically,
/// one source of truth for how a chunk becomes an `AiStoreEntry`.
fn build_entry(chunk: &CodeChunk) -> AiStoreEntry {
    let text = chunk.build_embedding_text();
    let mut meta_data = HashMap::new();
    let meta_data_list = vec![
        parse_metadata(Metadata::Name, &chunk.item_name),
        parse_metadata(Metadata::Kind, &format!("{}", chunk.kind)),
        parse_metadata(Metadata::Path, &chunk.file_path),
        parse_metadata(
            Metadata::Scope,
            &format!("{} {}", chunk.start_line, chunk.end_line),
        ),
        parse_metadata(Metadata::Hash, &chunk.content_hash),
        parse_metadata(
            Metadata::RawCode,
            &helper::preview_code(&chunk.raw_code, 15),
        ),
    ];
    meta_data.extend(meta_data_list);

    AiStoreEntry {
        key: Some(StoreInput {
            value: Some(AiStoreValue::RawString(text)),
        }),
        value: Some(StoreValue { value: meta_data }),
    }
}

/// Extracts the matched code chunks (as raw text) from an Ahnlich GetSimN response,
/// ordered by similarity (closest match first).
fn format_results(res: server::GetSimN) -> Vec<SimNHit> {
    res.entries
        .into_iter()
        .filter_map(|entry| SimNHit::try_from(entry).ok())
        .collect()
}

/// Converts allowed Metadata key-value pairs unto a tuple of Ahnlich's Metadata Hashmap value.
pub fn parse_metadata(key: Metadata, value: &str) -> (String, MetadataValue) {
    (
        key.to_string(),
        MetadataValue {
            value: Some(MetaValue::RawString(value.to_string())),
        },
    )
}
