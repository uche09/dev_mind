use colored::*;
use std::fmt::Display;

use crate::embeddings::metadata::Metadata;
use crate::utils::helper::preview_code;
use ahnlich_types::{
    ai::server::GetSimNEntry,
    metadata::{MetadataValue, metadata_value::Value},
};

/// A representation of a single match data from Ahnlich Similarity search result.
pub struct SimNHit {
    pub name: String,
    pub kind: String,
    pub path: String,
    pub similarity: f32,
    pub start_line: Option<u32>,
    pub end_line: Option<u32>,
    pub snippet: String,
}

impl TryFrom<GetSimNEntry> for SimNHit {
    type Error = anyhow::Error;

    /// Converts from Ahnlich Sim Search result type to defined representation type
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
            name: metadata_value_to_string(value.value.get(&Metadata::Name.to_string()))
                .unwrap_or_default(),
            kind: metadata_value_to_string(value.value.get(&Metadata::Kind.to_string()))
                .unwrap_or_default(),
            path: metadata_value_to_string(value.value.get(&Metadata::Path.to_string()))
                .unwrap_or_default(),
            snippet: metadata_value_to_string(value.value.get(&Metadata::RawCode.to_string()))
                .unwrap_or_default(),
            similarity,
            start_line,
            end_line,
        };

        Ok(new_hit)
    }
}

impl Display for SimNHit {
    /// Card display for a single match data from Ahnlich similarity search
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let header = format!("similarity {:.3} -------", self.similarity);
        let start_line = self.start_line.map_or("_".to_string(), |l| l.to_string());
        let end_line = self.end_line.map_or("_".to_string(), |l| l.to_string());

        write!(f, "{}", header.cyan())?;
        write!(
            f,
            "   {}  {}",
            format!("{}: {}-{}", self.path, start_line, end_line)
                .green()
                .bold(),
            format!("[{}]", self.kind).dimmed()
        )?;

        write!(f, " {}", self.name.yellow().bold())?;
        writeln!(f)?;

        for line in preview_code(&self.snippet, 6).lines() {
            writeln!(f, "     {}", line)?;
        }
        writeln!(f)
    }
}

fn metadata_value_to_string(mv: Option<&MetadataValue>) -> Option<String> {
    match mv {
        Some(mt) => match &mt.value {
            Some(Value::RawString(txt)) => Some(txt.to_owned()),
            None => None,
            _ => None,
        },

        None => None,
    }
}
