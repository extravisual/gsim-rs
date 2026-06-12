use criterion::{Criterion, criterion_group, criterion_main};
use gsim_rs::source::Source;

fn source() {
    // run from project dir
    let _ = Source::from_file("gcodes/adaptive.gcode").unwrap();
}

fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("source", |b| b.iter(|| source()));
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
