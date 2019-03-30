#[macro_use]
extern crate criterion;
extern crate evtx_rs;

use criterion::Criterion;
use evtx_rs::evtx::EvtxParser;

// first chunk has 90 records
fn process_90_records(buffer: &'static [u8]) {
    let parser = EvtxParser::from_buffer(buffer);

    for (i, record) in parser.records().take(90).enumerate() {
        match record {
            Ok(r) => {
                assert_eq!(r.event_record_id, i as u64 + 1);
            }
            Err(e) => println!("Error while reading record {}, {:?}", i, e),
        }
    }
}

fn criterion_benchmark(c: &mut Criterion) {
    let evtx_file = include_bytes!("../../samples/security.evtx");
    // ~11ms before strings cache
    // ~9ms after strings cache
    c.bench_function("read 90 records", move |b| {
        b.iter(|| process_90_records(evtx_file))
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);