use super::metadata::Metadata;
use crate::{parser::chunk::CodeChunk, search::SimNHit};
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

    pub async fn ping(&self) -> anyhow::Result<Pong> {
        Ok(self.client.ping(None).await?)
    }

    pub async fn create_store(&self) -> anyhow::Result<()> {
        self.client
            .create_store(
                CreateStore {
                    store: self.store.to_owned(),
                    query_model: AiModel::JinaEmbeddingsV2BaseCode as i32,
                    index_model: AiModel::JinaEmbeddingsV2BaseCode as i32,
                    predicates: vec![], // likely store file path and code lines later
                    non_linear_indices: vec![],
                    error_if_exists: false,
                    store_original: false,
                },
                None,
            )
            .await?;
        Ok(())
    }

    pub async fn add_chuck(&self, chunk: &CodeChunk) -> anyhow::Result<()> {
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
        ];
        meta_data.extend(meta_data_list.into_iter());

        let data_to_store = Set {
            store: self.store.clone(),
            inputs: vec![AiStoreEntry {
                key: Some(StoreInput {
                    value: Some(AiStoreValue::RawString(text)),
                }),
                value: Some(StoreValue { value: meta_data }),
            }],
            preprocess_action: PreprocessAction::NoPreprocessing as i32,
            execution_provider: None,
            model_params: HashMap::new(),
        };

        self.client.set(data_to_store, None).await?;

        Ok(())
    }

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

/// Extracts the matched code chunks (as raw text) from an Ahnlich GetSimN response,
/// ordered by similarity (closest match first).
fn format_results(res: server::GetSimN) -> Vec<SimNHit> {
    res.entries
        .into_iter()
        .filter_map(|entry| SimNHit::try_from(entry).ok())
        .collect()
}

pub fn parse_metadata(key: Metadata, value: &str) -> (String, MetadataValue) {
    (
        key.to_string(),
        MetadataValue {
            value: Some(MetaValue::RawString(value.to_string())),
        },
    )
}
