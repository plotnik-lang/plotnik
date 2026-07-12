use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use plotnik_lib::format_query;

fn nested_broken_query(depth: usize) -> String {
    let mut pattern = "(leaf)".to_owned();
    for _ in 0..depth {
        pattern = format!("(node {pattern} (sibling))");
    }
    format!("Q = {pattern}")
}

fn nested_field_query(depth: usize) -> String {
    let mut pattern = "(leaf)".to_owned();
    for _ in 0..depth {
        pattern = format!("field: {pattern}");
    }
    format!("Q = {pattern}")
}

fn flat_query(items: usize, commented: bool) -> String {
    let item = if commented {
        "/* comment */ (leaf)"
    } else {
        "(leaf)"
    };
    format!(
        "Q = (root {})",
        std::iter::repeat_n(item, items)
            .collect::<Vec<_>>()
            .join(" ")
    )
}

fn definitions_query(definitions: usize) -> String {
    (0..definitions)
        .map(|index| format!("Definition{index} = (leaf)"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn bench_series(c: &mut Criterion, name: &str, sizes: &[usize], build: impl Fn(usize) -> String) {
    let mut group = c.benchmark_group(name);
    for &size in sizes {
        let query = build(size);
        let output = format_query(&query).expect("benchmark query formats");
        group.throughput(Throughput::Bytes(query.len() as u64));
        group.bench_with_input(
            BenchmarkId::new(
                format!("n={size}"),
                format!("{}in-{}out", query.len(), output.len()),
            ),
            &query,
            |b, query| b.iter(|| format_query(query).expect("benchmark query formats")),
        );
    }
    group.finish();
}

fn formatter_scaling(c: &mut Criterion) {
    bench_series(
        c,
        "formatter/nested-broken",
        &[8, 16, 32, 48],
        nested_broken_query,
    );
    bench_series(
        c,
        "formatter/nested-fields",
        &[8, 32, 64, 96, 120],
        nested_field_query,
    );
    bench_series(
        c,
        "formatter/flat-items",
        &[100, 400, 1_600, 6_400],
        |size| flat_query(size, false),
    );
    bench_series(
        c,
        "formatter/inline-comments",
        &[100, 400, 1_600, 6_400],
        |size| flat_query(size, true),
    );
    bench_series(
        c,
        "formatter/definitions",
        &[10, 100, 1_000, 4_000],
        definitions_query,
    );
    bench_series(c, "formatter/unicode-text", &[16, 64, 256, 1_024], |size| {
        format!("Q = (identifier == \"{}\")", "λ🙂".repeat(size))
    });
    bench_series(
        c,
        "formatter/long-token",
        &[64, 256, 1_024, 4_096],
        |size| format!("Q = ({})", "a".repeat(size)),
    );
}

criterion_group!(benches, formatter_scaling);
criterion_main!(benches);
