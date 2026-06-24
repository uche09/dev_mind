use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct Config {
    pub store: String,
    #[serde(default = "default_ignore")]
    pub ignore: Vec<String>,
}

fn default_ignore() -> Vec<String> {
    vec!["**/target".to_string(), "**/*.toml".to_string()]
}

impl Config {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        if !std::path::Path::new(path).exists() {
            return Ok(Config {
                store: "mind_code".to_string(),
                ignore: default_ignore(),
            });
        }

        let text = std::fs::read_to_string(path)?;
        Ok(toml::from_str(&text)?)
    }
}
