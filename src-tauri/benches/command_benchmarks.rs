use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use std::fs;
use std::io::Write;
use std::process::Command;
use tempfile::NamedTempFile;

/// Helper to create markdown content of specified size
fn create_markdown_content(size_kb: usize) -> String {
    let base = "# Test Heading\n\nThis is a test paragraph with some content.\n\n";
    let target_size = size_kb * 1024;
    let repetitions = target_size / base.len() + 1;
    base.repeat(repetitions)[..target_size].to_string()
}

/// Benchmark: Small markdown file (< 10KB)
fn bench_markdown_small(c: &mut Criterion) {
    let mut group = c.benchmark_group("markdown");

    let sizes = vec![1, 5, 10]; // KB

    for size_kb in sizes {
        group.throughput(Throughput::Bytes((size_kb * 1024) as u64));

        group.bench_with_input(
            BenchmarkId::new("small_file", format!("{}KB", size_kb)),
            &size_kb,
            |b, &size| {
                // Create temp file with specified size
                let content = create_markdown_content(size);
                let mut temp_file = NamedTempFile::new().unwrap();
                write!(temp_file, "{}", content).unwrap();
                temp_file.flush().unwrap();
                let path = temp_file.path().to_path_buf();

                b.iter(|| {
                    let output = Command::new("cargo")
                        .args(&["run", "--release", "--", "markdown"])
                        .arg(&path)
                        .output()
                        .expect("Failed to execute command");

                    black_box(output);
                });
            },
        );
    }

    group.finish();
}

/// Benchmark: Medium markdown files (50KB - 500KB)
fn bench_markdown_medium(c: &mut Criterion) {
    let mut group = c.benchmark_group("markdown_medium");
    group.sample_size(10); // Reduce sample size for medium files

    let sizes = vec![50, 100, 500]; // KB

    for size_kb in sizes {
        group.throughput(Throughput::Bytes((size_kb * 1024) as u64));

        group.bench_with_input(
            BenchmarkId::new("medium_file", format!("{}KB", size_kb)),
            &size_kb,
            |b, &size| {
                let content = create_markdown_content(size);
                let mut temp_file = NamedTempFile::new().unwrap();
                write!(temp_file, "{}", content).unwrap();
                temp_file.flush().unwrap();
                let path = temp_file.path().to_path_buf();

                b.iter(|| {
                    let output = Command::new("cargo")
                        .args(&["run", "--release", "--", "markdown"])
                        .arg(&path)
                        .output()
                        .expect("Failed to execute command");

                    black_box(output);
                });
            },
        );
    }

    group.finish();
}

/// Benchmark: Large markdown files (1MB - 2MB)
fn bench_markdown_large(c: &mut Criterion) {
    let mut group = c.benchmark_group("markdown_large");
    group.sample_size(10); // Reduce sample size for large files

    let sizes = vec![1000, 1900]; // KB (1MB, ~2MB)

    for size_kb in sizes {
        group.throughput(Throughput::Bytes((size_kb * 1024) as u64));

        group.bench_with_input(
            BenchmarkId::new("large_file", format!("{}KB", size_kb)),
            &size_kb,
            |b, &size| {
                let content = create_markdown_content(size);
                let mut temp_file = NamedTempFile::new().unwrap();
                write!(temp_file, "{}", content).unwrap();
                temp_file.flush().unwrap();
                let path = temp_file.path().to_path_buf();

                b.iter(|| {
                    let output = Command::new("cargo")
                        .args(&["run", "--release", "--", "markdown"])
                        .arg(&path)
                        .output()
                        .expect("Failed to execute command");

                    black_box(output);
                });
            },
        );
    }

    group.finish();
}

/// Benchmark: Image processing with Kitty protocol
fn bench_image_kitty(c: &mut Criterion) {
    let mut group = c.benchmark_group("image_kitty");
    group.sample_size(10);

    // Use existing test fixtures
    let test_images = vec![
        ("small.png", "tests/fixtures/small.png"),
        ("photo.jpg", "tests/fixtures/photo.jpg"),
        ("animation.gif", "tests/fixtures/animation.gif"),
        ("graphic.webp", "tests/fixtures/graphic.webp"),
    ];

    for (name, path) in test_images {
        // Get file size for throughput measurement
        if let Ok(metadata) = fs::metadata(path) {
            let size = metadata.len();
            group.throughput(Throughput::Bytes(size));

            group.bench_with_input(BenchmarkId::new("kitty", name), path, |b, path| {
                b.iter(|| {
                    let output = Command::new("cargo")
                        .args(&["run", "--release", "--", "image"])
                        .arg(path)
                        .output()
                        .expect("Failed to execute command");

                    black_box(output);
                });
            });
        }
    }

    group.finish();
}

/// Benchmark: Image processing with SIXEL protocol
fn bench_image_sixel(c: &mut Criterion) {
    let mut group = c.benchmark_group("image_sixel");
    group.sample_size(10);

    let test_images = vec![
        ("small.png", "tests/fixtures/small.png"),
        ("photo.jpg", "tests/fixtures/photo.jpg"),
    ];

    for (name, path) in test_images {
        if let Ok(metadata) = fs::metadata(path) {
            let size = metadata.len();
            group.throughput(Throughput::Bytes(size));

            group.bench_with_input(BenchmarkId::new("sixel", name), path, |b, path| {
                b.iter(|| {
                    let output = Command::new("cargo")
                        .args(&["run", "--release", "--", "image"])
                        .arg("--protocol")
                        .arg("sixel")
                        .arg(path)
                        .output()
                        .expect("Failed to execute command");

                    black_box(output);
                });
            });
        }
    }

    group.finish();
}

/// Benchmark: Base64 encoding performance
fn bench_base64_encoding(c: &mut Criterion) {
    use base64::{Engine as _, engine::general_purpose};

    let mut group = c.benchmark_group("base64_encoding");

    let sizes = vec![1024, 10240, 102400, 1048576]; // 1KB, 10KB, 100KB, 1MB

    for size in sizes {
        group.throughput(Throughput::Bytes(size as u64));

        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            let data = vec![0u8; size];

            b.iter(|| {
                let encoded = general_purpose::STANDARD.encode(black_box(&data));
                black_box(encoded);
            });
        });
    }

    group.finish();
}

/// Benchmark: File reading performance
fn bench_file_reading(c: &mut Criterion) {
    let mut group = c.benchmark_group("file_reading");

    let sizes = vec![1024, 10240, 102400, 1048576, 2097152]; // 1KB to 2MB

    for size in sizes {
        group.throughput(Throughput::Bytes(size as u64));

        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            // Create temp file
            let mut temp_file = NamedTempFile::new().unwrap();
            let data = vec![0u8; size];
            temp_file.write_all(&data).unwrap();
            temp_file.flush().unwrap();
            let path = temp_file.path().to_path_buf();

            b.iter(|| {
                let contents = fs::read(black_box(&path)).expect("Failed to read file");
                black_box(contents);
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_markdown_small,
    bench_markdown_medium,
    bench_markdown_large,
    bench_image_kitty,
    bench_image_sixel,
    bench_base64_encoding,
    bench_file_reading
);

criterion_main!(benches);
