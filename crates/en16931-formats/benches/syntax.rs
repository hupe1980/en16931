//! How long reading and writing take, measured rather than asserted.
//!
//! # Why this exists
//!
//! The model crate's benchmark exists because `README.md` claims microseconds
//! rather than milliseconds, and a performance claim in a project that spends a
//! section on the cost of unverified claims needs a number. **This crate made no
//! such claim, and that is worse**: reading is where a batch job actually spends
//! its time — a receiver is handed a document, not a model — and there was no
//! figure for it at all. An absent number cannot be wrong, and it cannot be
//! relied on either.
//!
//! The comparisons worth having:
//!
//! * `read/ubl` against `read/cii` — the two mandatory syntaxes on the same
//!   invoice, so a caller choosing one knows what it costs.
//! * `read/*` against `write/*` — reading is the hard direction and should show
//!   it; a writer slower than a reader is doing something it should not.
//! * `read/ubl/1000` — whether reading stays **linear** in line count. The rules
//!   are (the model crate's benchmark asserts it); a reader that goes quadratic
//!   is fine on examples and dies on a telecoms bill with 5 000 call records.
//! * `convert/ubl-to-cii` — read, model, write: what `en16931 convert` does, and
//!   the only figure a user of that command has.
//!
//! Run with `cargo bench -p en16931-formats --all-features`.

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use en16931::invoice::*;
use en16931::{Date, InvoiceAmount, Percentage, Quantity};
use rust_decimal::Decimal;
use std::hint::black_box;

fn party(name: &str) -> Party {
    Party {
        name: Some(name.to_owned()),
        vat_identifier: Some("DE123456789".into()),
        electronic_address: Some(en16931::Identifier::schemed("991-01234-56", "0204")),
        address: PostalAddress {
            city: Some("Musterstadt".into()),
            post_code: Some("12345".into()),
            country: Some(Code::new("DE")),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// An invoice with `n` lines at two VAT rates — the shape a real one has.
fn invoice(n: usize) -> Invoice {
    // `Invoice` is `#[non_exhaustive]`, so it is filled in rather than
    // constructed — which is the point of that attribute.
    let mut inv = Invoice::default();
    inv.specification_id = Some("urn:cen.eu:en16931:2017".into());
    inv.number = Some("R-2026-0001".into());
    inv.issue_date = Date::parse("2026-07-31").ok();
    inv.type_code = Some(Code::new("380"));
    inv.currency = Some(Code::new("EUR"));
    inv.seller = party("Stadtwerke Musterstadt GmbH");
    inv.buyer = party("Beispiel AG");
    inv.due_date = Date::parse("2026-08-14").ok();

    for i in 0..n {
        inv.lines.push(InvoiceLine::new(
            (i + 1).to_string(),
            "Netznutzung Arbeitspreis",
            Quantity::new(Decimal::from(1000)),
            "KWH",
            InvoiceAmount::parse("289.00").expect("amount"),
            "S",
            Some(Percentage::new(Decimal::from(if i % 2 == 0 {
                19
            } else {
                7
            }))),
        ));
    }
    // The totals a real document carries, so the reader has them to parse.
    let _ = en16931::reconcile(&mut inv);
    inv
}

fn benches(c: &mut Criterion) {
    // Reading: the direction a receiver takes, and the one all 1 339 syntax
    // rules apply to.
    let mut group = c.benchmark_group("read");
    for n in [5usize, 100, 1000] {
        let inv = invoice(n);
        {
            let xml = en16931_formats::ubl::to_string(&inv);
            group.bench_with_input(BenchmarkId::new("ubl", n), &xml, |b, xml| {
                b.iter(|| en16931_formats::ubl::from_str(black_box(xml)).expect("readable"));
            });
        }
        {
            let xml = en16931_formats::cii::to_string(&inv);
            group.bench_with_input(BenchmarkId::new("cii", n), &xml, |b, xml| {
                b.iter(|| en16931_formats::cii::from_str(black_box(xml)).expect("readable"));
            });
        }
    }
    group.finish();

    // Writing cannot fail and does no I/O, so this is the element-order sort
    // and the string building, and nothing else.
    let mut group = c.benchmark_group("write");
    for n in [5usize, 1000] {
        let inv = invoice(n);
        group.bench_with_input(BenchmarkId::new("ubl", n), &inv, |b, inv| {
            b.iter(|| en16931_formats::ubl::to_string(black_box(inv)));
        });
        group.bench_with_input(BenchmarkId::new("cii", n), &inv, |b, inv| {
            b.iter(|| en16931_formats::cii::to_string(black_box(inv)));
        });
    }
    group.finish();

    // `en16931 convert`, end to end: read, hold the model, write the other
    // syntax — the only number a user of that command has.
    {
        let mut group = c.benchmark_group("convert");
        let inv = invoice(5);
        let ubl = en16931_formats::ubl::to_string(&inv);
        group.bench_function("ubl-to-cii", |b| {
            b.iter(|| {
                let read = en16931_formats::ubl::from_str(black_box(&ubl)).expect("readable");
                en16931_formats::cii::to_string(&read.invoice)
            });
        });
        let cii = en16931_formats::cii::to_string(&inv);
        group.bench_function("cii-to-ubl", |b| {
            b.iter(|| {
                let read = en16931_formats::cii::from_str(black_box(&cii)).expect("readable");
                en16931_formats::ubl::to_string(&read.invoice)
            });
        });
        group.finish();
    }
}

criterion_group!(syntax, benches);
criterion_main!(syntax);
