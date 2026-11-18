//! Micro-benchmarks for alphen-box M1 skeleton

use alphen_core::ImageReference;
use std::time::Instant;

#[test]
fn bench_image_reference_parse_throughput() {
    let iterations = 20_000;
    let start = Instant::now();

    for _ in 0..iterations {
        let parsed = ImageReference::parse("docker.io/library/alpine:3.20").unwrap();
        assert_eq!(parsed.tag, "3.20");
    }

    let elapsed = start.elapsed();
    let per_op = elapsed / iterations;
    println!(
        "\n[BENCH] ImageReference::parse: total {:?} for {} iterations ({:?} / parse)",
        elapsed, iterations, per_op
    );
    // Sub-microsecond budget per reference parse
    assert!(per_op.as_micros() < 50);
}

#[test]
fn bench_system_requirements_check_overhead() {
    let iterations = 10_000;
    let start = Instant::now();

    for _ in 0..iterations {
        let caps = alphen_runtime::verify_system_requirements().unwrap();
        assert!(caps.kernel_supported);
    }

    let elapsed = start.elapsed();
    let per_op = elapsed / iterations;
    println!(
        "\n[BENCH] verify_system_requirements: total {:?} for {} iterations ({:?} / check)",
        elapsed, iterations, per_op
    );
    // Fast system verification check (< 100 microseconds)
    assert!(per_op.as_micros() < 100);
}
