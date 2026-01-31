//! Benchmark harness for find_image optimizations.
//!
//! Run with:
//! ```sh
//! # Baseline (no optimizations)
//! cargo bench --bench find_image_bench --no-default-features
//!
//! # With parallel processing only
//! cargo bench --bench find_image_bench --no-default-features --features find_image_parallel
//!
//! # With SIMD only
//! cargo bench --bench find_image_bench --no-default-features --features find_image_simd
//!
//! # With all optimizations (default)
//! cargo bench --bench find_image_bench
//! ```

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use image::{GrayImage, Luma};
use native_devtools_mcp::tools::find_image::{compute_template_stats, match_template_ncc};

/// Build a synthetic grayscale image with a gradient pattern.
fn build_image(w: u32, h: u32) -> GrayImage {
    GrayImage::from_fn(w, h, |x, y| {
        // Create a pattern with variation for NCC to work with
        let val = ((x.wrapping_mul(3).wrapping_add(y.wrapping_mul(7))) % 255) as u8;
        Luma([val])
    })
}

/// Build a synthetic template image with a distinct pattern.
fn build_template(w: u32, h: u32) -> GrayImage {
    GrayImage::from_fn(w, h, |x, y| {
        // Different pattern from the main image
        let val = ((x.wrapping_mul(11).wrapping_add(y.wrapping_mul(13))) % 255) as u8;
        Luma([val])
    })
}

/// Benchmark match_template_ncc with various screen and template sizes.
fn bench_match_template_ncc(c: &mut Criterion) {
    let mut group = c.benchmark_group("match_template_ncc");

    // Screen sizes to test
    let screen_sizes: Vec<(u32, u32)> = vec![
        (1920, 1080), // Full HD
        (2560, 1440), // QHD
    ];

    // Template sizes to test
    let template_sizes: Vec<(u32, u32)> = vec![
        (24, 24),
        (32, 32),
        (64, 64),
    ];

    // Strides to test (2 = fast mode default)
    let stride = 2u32;

    // Threshold (fast mode default)
    let threshold = 0.88;

    for (sw, sh) in &screen_sizes {
        let screen = build_image(*sw, *sh);

        for (tw, th) in &template_sizes {
            let template = build_template(*tw, *th);

            // Set throughput to number of positions checked
            let search_w = sw - tw + 1;
            let search_h = sh - th + 1;
            let positions = ((search_w / stride) * (search_h / stride)) as u64;
            group.throughput(Throughput::Elements(positions));

            let bench_id = BenchmarkId::new(
                format!("{}x{}", sw, sh),
                format!("{}x{}_stride{}", tw, th, stride),
            );

            group.bench_with_input(bench_id, &(&screen, &template), |b, (screen, template)| {
                b.iter(|| {
                    black_box(match_template_ncc(screen, template, None, threshold, stride))
                });
            });
        }
    }

    group.finish();
}

/// Benchmark template stats computation (used for pre-computation).
fn bench_template_stats(c: &mut Criterion) {
    let mut group = c.benchmark_group("compute_template_stats");

    let template_sizes: Vec<(u32, u32)> = vec![
        (24, 24),
        (32, 32),
        (64, 64),
        (128, 128),
    ];

    for (tw, th) in template_sizes {
        let template = build_template(tw, th);
        let bench_id = BenchmarkId::new("template", format!("{}x{}", tw, th));

        group.bench_with_input(bench_id, &template, |b, template| {
            b.iter(|| black_box(compute_template_stats(template, None)));
        });
    }

    group.finish();
}

/// Benchmark with different stride values.
fn bench_stride_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("stride_comparison");

    let screen = build_image(1920, 1080);
    let template = build_template(32, 32);
    let threshold = 0.88;

    for stride in [1u32, 2, 4] {
        let bench_id = BenchmarkId::new("1920x1080_32x32", format!("stride{}", stride));

        group.bench_with_input(bench_id, &stride, |b, &stride| {
            b.iter(|| black_box(match_template_ncc(&screen, &template, None, threshold, stride)));
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_match_template_ncc,
    bench_template_stats,
    bench_stride_comparison,
);
criterion_main!(benches);
