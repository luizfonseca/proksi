use std::{collections::HashMap, sync::Arc};

use arc_swap::ArcSwap;
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use dashmap::DashMap;

#[derive(Clone)]
struct ComplexStruct {
    #[allow(dead_code)]
    string: String,
    #[allow(dead_code)]
    arc: Option<Arc<ComplexStruct>>,
    #[allow(dead_code)]
    map: HashMap<String, ComplexStruct>,
}

impl Default for ComplexStruct {
    fn default() -> Self {
        Self {
            string: "hello".to_string(),
            arc: Some(Arc::new(ComplexStruct {
                arc: None,
                map: HashMap::new(),
                string: "hello".to_string(),
            })),
            map: HashMap::new(),
        }
    }
}

fn arc_swap_read_from_hashmap(_dummy: usize) {
    let mut pointee = HashMap::new();

    // Insert 100 items
    for i in 0..100 {
        pointee.insert(i.to_string(), ComplexStruct::default());
    }

    pointee.insert("default".to_string(), ComplexStruct::default());
    let arc_swap = ArcSwap::from_pointee(pointee);

    // Read from the arcswap
    for i in 0..100 {
        let v = arc_swap.load();
        v.get(&i.to_string()).unwrap();
    }

    // Update the pointee through the arcswap
    arc_swap.rcu(|v| {
        let mut p = (**v).clone();
        p.insert("default-2".to_string(), ComplexStruct::default());
        p
    });

    // ensure we have the new value
    let v = arc_swap.load();
    assert!(v.get("default-2").is_some());
}

fn dashmap_arc_read_from_hashmap(_dummy: usize) {
    let pointee = DashMap::new();

    // Insert 100 items
    for i in 0..100 {
        pointee.insert(i.to_string(), ComplexStruct::default());
    }

    pointee.insert("default".to_string(), ComplexStruct::default());
    let arc_swap = Arc::new(pointee);

    // Read from the arcswap
    for i in 0..100 {
        let v = arc_swap.clone();
        v.get(&i.to_string()).unwrap();
    }

    // Update the pointee through the arcswap
    arc_swap
        .clone()
        .insert("default-2".to_string(), ComplexStruct::default());

    // ensure we have the new value
    let v = arc_swap;
    assert!(v.get("default-2").is_some());
}

fn criterion_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("Dashmap vs ArcSwap");

    group.bench_function("arc_swap_read_from_hashmap", |b| {
        b.iter(|| arc_swap_read_from_hashmap(black_box(100)))
    });

    group.bench_function("dashmap_read_from_hashmap", |b| {
        b.iter(|| dashmap_arc_read_from_hashmap(black_box(100)))
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
