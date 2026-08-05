use crate::embeddings::metadata::Metadata;
use ahnlich_types::{
    ai::server::GetSimNEntry,
    metadata::{MetadataValue, metadata_value::Value},
};

pub struct SimNHit {
    pub kind: String,
    pub path: String,
    pub similarity: f32,
    pub start_line: Option<u32>,
    pub end_line: Option<u32>,
}

impl TryFrom<GetSimNEntry> for SimNHit {
    type Error = anyhow::Error;
    fn try_from(value: GetSimNEntry) -> Result<Self, Self::Error> {
        let similarity: f32 = value.similarity.unwrap_or_default().value;
        let value = value.value.ok_or(anyhow::Error::msg("No metadata"))?;
        let scope = metadata_value_to_string(value.value.get(&Metadata::Scope.to_string())).map_or(
            vec![0, 0],
            |scope| {
                scope
                    .split(" ")
                    .map(|n| n.parse::<u32>().unwrap_or_default())
                    .collect()
            },
        );

        let (start_line, end_line);

        if scope[0] == 0 && scope[1] == 0 {
            start_line = None;
            end_line = None
        } else {
            start_line = Some(scope[0]);
            end_line = Some(scope[1]);
        }

        let new_hit = Self {
            kind: metadata_value_to_string(value.value.get(&Metadata::Kind.to_string()))
                .unwrap_or_default(),
            path: metadata_value_to_string(value.value.get(&Metadata::Path.to_string()))
                .unwrap_or_default(),
            similarity,
            start_line,
            end_line,
        };

        Ok(new_hit)
    }
}

fn metadata_value_to_string(mv: Option<&MetadataValue>) -> Option<String> {
    match mv {
        Some(mt) => match &mt.value {
            Some(Value::RawString(txt)) => {
                return Some(txt.to_owned());
            }
            None => {
                return None;
            }
            _ => {
                return None;
            }
        },

        None => return None,
    }
}
