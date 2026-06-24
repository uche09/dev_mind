use anyhow;
use globset::{Glob, GlobSet, GlobSetBuilder};
use walkdir::{DirEntry, WalkDir};

pub fn build_ignore_set(patterns: &[String]) -> anyhow::Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();

    for pattern in patterns {
        builder.add(Glob::new(pattern)?);
    }

    Ok(builder.build()?)
}

fn is_ignored(entry: &DirEntry, ignore: &GlobSet) -> bool {
    let rel = entry.path();
    ignore.is_match(rel)
}

// This function traverses the directory path provided.
pub fn collect_rust_files(root: &str, ignore: &GlobSet) -> anyhow::Result<Vec<String>> {
    let root_path = std::path::Path::new(root);

    if !root_path.exists() || !root_path.is_dir() {
        anyhow::bail!("expected a directory")
    }

    Ok(WalkDir::new(root_path)
        .into_iter()
        .filter_entry(|e| !is_ignored(e, ignore))
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "rs"))
        .map(|e| e.path().to_string_lossy().to_string())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn ignore_pattern_match_target_dir() -> anyhow::Result<()> {
        let set = build_ignore_set(&["**/target".to_string(), "**/*.toml".to_string()])?;

        assert!(set.is_match("target"));
        assert!(set.is_match("nested/target"));
        assert!(!set.is_match("target/debug")); // doesn't match contents, only the dir itself
        assert!(set.is_match("mind.toml"));
        assert!(set.is_match("src/mind.toml"));

        Ok(())
    }

    #[test]
    fn returns_rust_files() {
        let root = env!("CARGO_MANIFEST_DIR");
        let config = Config::load(&format!("{}/mind.toml", root)).unwrap();
        let ignore = build_ignore_set(&config.ignore).unwrap();

        let rust_files = collect_rust_files(root, &ignore).unwrap();

        assert!(rust_files.contains(&format!("{root}/src/main.rs")));
        assert!(rust_files.contains(&format!("{root}/src/config.rs")));
        assert!(!rust_files.contains(&format!("{root}/mind.toml")));
        assert!(!rust_files.contains(&format!("{root}/Cargo.toml")));
        
    }
}
