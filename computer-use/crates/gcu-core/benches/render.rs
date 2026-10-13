#![allow(missing_docs)]
#[path = "../tests/support/mod.rs"]
mod support;
use criterion::{criterion_group, criterion_main, Criterion};
use gcu_core::{driver::TreeOpts, model::Role, render::render_text, tree::Pipeline};
use std::hint::black_box;

fn bench_render(c: &mut Criterion) {
    let input = support::raw(
        (0..4999)
            .map(|i| support::node(Role::Row, &format!("Item {i}"), vec![]))
            .collect(),
    );
    let tree = Pipeline::default().run(
        input,
        &TreeOpts {
            max_tree_nodes: 5000,
            ..Default::default()
        },
    );
    assert_eq!(tree.element_index.len(), 5000);
    let opts = support::opts();
    c.bench_function("render_text_5000_nodes", |b| {
        b.iter(|| render_text(black_box(&tree), black_box(&opts)))
    });
}
criterion_group!(benches, bench_render);
criterion_main!(benches);
