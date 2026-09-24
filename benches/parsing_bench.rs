use criterion::{Criterion, black_box, criterion_group, criterion_main};
use devmind::parser::parse_file;
use devmind::parser::traverser::{build_ignore_set, collect_rust_files};

fn default_ignore() -> globset::GlobSet {
    build_ignore_set(&["**/target".to_string(), "**/*.toml".to_string()]).unwrap()
}

fn bench_collect_rust_files(c: &mut Criterion) {
    let root = env!("CARGO_MANIFEST_DIR");
    let ignore = default_ignore();

    c.bench_function("collect_rust_files (this project)", |b| {
        b.iter(|| collect_rust_files(black_box(root), black_box(&ignore)).unwrap())
    });
}

fn bench_parse_single_file(c: &mut Criterion) {
    let root = env!("CARGO_MANIFEST_DIR");
    // chunk.rs is a real, moderately sized file in this project repo, a
    // representative single-file parse rather than a synthetic snippet.
    let target_file = format!("{root}/src/parser/chunk.rs");

    c.bench_function("parse_file (chunk.rs)", |b| {
        b.iter(|| parse_file(black_box(&target_file)).unwrap())
    });
}

fn bench_parse_all_files(c: &mut Criterion) {
    let root = env!("CARGO_MANIFEST_DIR");
    let ignore = default_ignore();
    let files = collect_rust_files(root, &ignore).unwrap();

    let mut group = c.benchmark_group("parse_all_files");
    // Fewer samples: parsing every file in the project is heavier per
    // iteration than parsing one file, keep total bench runtime reasonable.
    group.sample_size(20);

    group.bench_function("parse_file (every file in this project)", |b| {
        b.iter(|| {
            for file in &files {
                let _ = parse_file(black_box(file)).unwrap();
            }
        })
    });

    group.finish();
}

/// Optional: point this at a bigger checkout (e.g. a local clone of the
/// Ahnlich repo) via an env var, to reproduce the same stress test carried out manually.
/// Skips itself (prints a note, doesn't fail) if the env var
/// isn't set, so `cargo bench` still works for everyone else without it.
fn bench_parse_external_repo(c: &mut Criterion) {
    let Ok(path) = std::env::var("DEVMIND_BENCH_PATH") else {
        println!(
            "Skipping bench_parse_external_repo: set DEVMIND_BENCH_PATH=/path/to/ahnlich to run it"
        );
        return;
    };

    let ignore = default_ignore();
    println!("Project path: {path}");
    let files = collect_rust_files(&path, &ignore).unwrap();
    println!("DEVMIND_BENCH_PATH: parsing {} files", files.len());

    let mut group = c.benchmark_group("parse_external_repo");
    group.sample_size(10);

    group.bench_function("parse_file (DEVMIND_BENCH_PATH)", |b| {
        b.iter(|| {
            for file in &files {
                let _ = parse_file(black_box(file)).unwrap();
            }
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_collect_rust_files,
    bench_parse_single_file,
    bench_parse_all_files,
    bench_parse_external_repo
);
criterion_main!(benches);
