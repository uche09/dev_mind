use serde::Deserialize;
use std::path::{Path, PathBuf};
use directories::ProjectDirs;
use colored::*;
use crate::cli::ConfigScope;

#[derive(Deserialize, Debug, Default)]
struct PartialConfig {
    store: Option<String>,
    ignore: Option<Vec<String>>,
    #[serde(alias = "ahnlich-addr")]
    ahnlich_addr: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub store: String,
    pub ignore: Vec<String>,
    pub ahnlich_addr: String,
}


impl Config {
    // Create default config files
    pub fn init(
        scope: ConfigScope,
        ahnlich_addr: Option<String>,
        store: Option<String>,
        ignore: Vec<String>,
        force: bool,
    ) -> anyhow::Result<()> {
        let defaults = Self::default();

        let addr = ahnlich_addr.unwrap_or_else(|| defaults.ahnlich_addr.clone().unwrap());
        let store_name = store.unwrap_or_else(|| defaults.store.clone().unwrap());
        let ignore_patterns = if ignore.is_empty() {
            defaults.ignore.clone().unwrap()
        } else {
            ignore
        };

        if matches! (scope, ConfigScope::Global | ConfigScope::Both) {
            let path = Self::global_config_path()
                .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?;
            Self::write_template(&path, &Self::global_template(&addr), force)?;
        }

        if matches! (scope, ConfigScope::Project | ConfigScope::Both) {
            let path = std::env::current_dir()?.join("devmind.toml");
            Self::write_template(
                &path, 
                &Self::project_template(&store_name, &ignore_patterns),
                force
            )?;
        }

        Ok(())
    }

    fn write_template(path: &Path, content: &str, force: bool) -> anyhow::Result<()> {
        if path.exists() && !force {
            println!(
                "{}",
                format!("Skipped {} (already exists, use --force to overwrite)", path.display()).yellow()
            );
            return Ok(());
        }

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Atomic write pattern: temp file, then rename,
        // so that a killed process never leaves a half-written config behind.
        let tmp_path = path.with_extension("toml.tmp");
        std::fs::write(&tmp_path, content)?;
        std::fs::rename(&tmp_path, path)?;

        println!("{}", format!("Wrote {}", path.display()).green());
        Ok(())
    }

    fn global_template(ahnlich_addr: &str) -> String {
        format!(
r#"# DevMind global config
# Applies to every project unless overridden by a project-level devmind.toml
# Location: resolved via XDG_CONFIG_HOME (~/.config/devmind/config.toml on Linux)

ahnlich_addr = "{ahnlich_addr}"
"#
        )
    }
    fn project_template(store: &str, ignore: &[String]) -> String {
        let ignore_list = ignore
            .iter()
            .map(|p| format!("\"{p}\""))
            .collect::<Vec<_>>()
            .join(", ");

        format!(
r#"# DevMind project config
# Overrides the global config for this project only.
# Safe to commit to version control.

store = "{store}"
ignore = [{ignore_list}]

# Uncomment to point this project at a different Ahnlich instance than
# your global default:
# ahnlich-addr = "localhost:1370"
"#
        )
    }
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
