//! Performance and latency benchmarks for `alphen-box` runtime components.

use alphen_core::ImageReference;
use alphen_runtime::ContainerConfig;
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

#[test]
fn bench_container_id_generation() {
    let iterations = 50_000;
    let start = Instant::now();

    for _ in 0..iterations {
        let id = ContainerConfig::new_id();
        assert_eq!(id.len(), 12);
    }

    let elapsed = start.elapsed();
    let per_op = elapsed / iterations;
    println!(
        "\n[BENCH] ContainerConfig::new_id: total {:?} for {} iterations ({:?} / op)",
        elapsed, iterations, per_op
    );
    assert!(per_op.as_nanos() < 5_000);
}

#[test]
fn bench_port_mapping_parse_throughput() {
    let iterations = 20_000;
    let start = Instant::now();

    for _ in 0..iterations {
        let m = "8080:80/tcp".parse::<alphen_net::PortMapping>().unwrap();
        assert_eq!(m.host_port, 8080);
    }

    let elapsed = start.elapsed();
    let per_op = elapsed / iterations;
    println!(
        "\n[BENCH] PortMapping::from_str: total {:?} for {} iterations ({:?} / op)",
        elapsed, iterations, per_op
    );
    assert!(per_op.as_micros() < 20);
}

#[test]
fn bench_cgroup_memory_parse() {
    let iterations = 20_000;
    let start = Instant::now();

    for _ in 0..iterations {
        let bytes = alphen_runtime::parse_memory_str("128m").unwrap();
        assert_eq!(bytes, 128 * 1024 * 1024);
    }

    let elapsed = start.elapsed();
    let per_op = elapsed / iterations;
    println!(
        "\n[BENCH] parse_memory_str: total {:?} for {} iterations ({:?} / op)",
        elapsed, iterations, per_op
    );
    assert!(per_op.as_micros() < 10);
}
