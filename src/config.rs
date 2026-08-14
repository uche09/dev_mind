use serde::Deserialize;
use std::path::{Path, PathBuf};
use directories::ProjectDirs;

#[derive(Deserialize, Debug, Default)]
struct PartialConfig {
    store: Option<String>,
    ignore: Option<Vec<String>>,
    ahnlich_addr: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub store: String,
    pub ignore: Vec<String>,
    pub ahnlich_addr: String,
}


impl Config {
    fn default() -> PartialConfig {
        PartialConfig { 
            store: Some("devmind".to_string()),
            ignore: Some(vec!["**/target".into(), "**/*.toml".into()]),
            ahnlich_addr: Some("localhost:1370".to_string()),
        }
    }
    fn read_layer(path: &Path) -> anyhow::Result<Option<PartialConfig>> {
        if !path.exists() {
            return Ok(None);
        }
        let text = std::fs::read_to_string(path)?;
        Ok(Some(toml::from_str(&text)?))
    }
    fn global_config_path() -> Option<PathBuf> {
        ProjectDirs::from("dev", "uche09", "devmind")
            .map(|dirs| dirs.config_dir().join("config.toml"))
    }

    // Walks upward from `start` to find devmind.toml project config file.
    // Same technique cargo and git uses to find Cargo.toml and .git files respectively
    fn find_project_config(start: &Path) -> Option<PathBuf> {
        let mut dir = Some(start);
        while let Some(d) = dir {
            let candidate = d.join("devmind.toml");
            if candidate.exists() {
                return Some(candidate);
            }
            dir = d.parent();
        }
        None
    }
    fn merge(base: PartialConfig, override_layer: PartialConfig) -> PartialConfig {
        PartialConfig { 
            store: override_layer.store.or(base.store), 
            ignore: override_layer.ignore.or(base.ignore), 
            ahnlich_addr: override_layer.ahnlich_addr.or(base.ahnlich_addr),
        }
    }
    pub fn load(config_path: Option<&Path>) -> anyhow::Result<Self> {
        let mut resolved = Self::default();

        // Layer 2: global (~/.config/devmind/config.toml)
        if let Some(path) = Self::global_config_path() 
        && 
        let Some(layer) = Self::read_layer(&path)? {
            resolved = Self::merge(resolved, layer);
            
        }

        // Layer 3: Project (nearest devmind.toml walking up from CWD)
        let cwd = std::env::current_dir()?;
        if let Some(path) = Self::find_project_config(&cwd)
        && let Some(layer) = Self::read_layer(&path)? {
            resolved = Self::merge(resolved, layer);
        }

        // Layer 4: Env var
        if let Ok(env_path) = std::env::var("DEVMIND_CONFIG")
        && let Some(layer) = Self::read_layer(Path::new(&env_path))? {
            resolved = Self::merge(resolved, layer);
        }

        // Layer 5: --config flag, must exist if given, else fail loudly
        if let Some(path) = config_path {
            match Self::read_layer(path)? {
                Some(layer) => resolved = Self::merge(resolved, layer),
                None => anyhow::bail!("Config file not found: {}", path.display())
            }
        }
        
        // defaults() guarantees every field is Some, so these unwraps can never panic;
        Ok(Config { 
            store: resolved.store.unwrap(),
            ignore: resolved.ignore.unwrap(),
            ahnlich_addr: resolved.ahnlich_addr.unwrap(),
        })
    }
}
