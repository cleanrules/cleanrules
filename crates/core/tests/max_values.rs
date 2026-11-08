//! The `max_values` limit on native data (`spec/data-model.md`, *Reading
//! native data*, *Limits*), with a reader of shared native objects as a
//! binding writes one: a diamond of depth 30 (2^30 references) is refused
//! within the limit, on a small stack, instead of exhausting memory.

#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use std::sync::Arc;

use cleanrules::json::Limits;
use cleanrules::value::{ValueCount, too_many_values};
use cleanrules::{Error, ErrorCode, Value};
use common::on_small_stack;

/// A native object graph, whose lists share their elements.
enum Native {
    Int(i64),
    List(Vec<Arc<Native>>),
}

/// A list holding `inner` twice, `depth` times over: 2^depth references.
fn diamond(depth: usize) -> Arc<Native> {
    (0..depth).fold(Arc::new(Native::Int(1)), |inner, _| {
        Arc::new(Native::List(vec![Arc::clone(&inner), inner]))
    })
}

/// Convert a value whole, counting each value as a binding does where a path
/// ends: a list's length is checked before it is allocated.
fn convert(native: &Native, count: &mut ValueCount) -> Result<Value, Error> {
    count.add(1)?;
    Ok(match native {
        Native::Int(n) => Value::Int(*n),
        Native::List(items) => {
            if items.len() > count.remaining() {
                return Err(too_many_values(count.limit()));
            }
            let mut list = Vec::with_capacity(items.len());
            for item in items {
                list.push(convert(item, count)?);
            }
            Value::List(list)
        }
    })
}

#[test]
fn the_default_is_one_million() {
    assert_eq!(Limits::default().max_values, 1_000_000);
    let mut limits = Limits::default();
    limits.max_values = 5;
    assert_eq!(limits.max_values, 5);
}

#[test]
fn a_count_stops_at_the_limit_and_names_it() {
    let mut count = ValueCount::new(3);
    count.add(1).unwrap();
    count.add(2).unwrap();
    assert_eq!((count.count(), count.remaining(), count.limit()), (3, 0, 3));
    let error = count.add(1).unwrap_err();
    assert_eq!(count.count(), 3);
    assert_eq!(error.code(), ErrorCode::LimitExceeded);
    assert_eq!(error.context().name.as_deref(), Some("max_values"));
    assert_eq!(
        error.message(),
        "the native data holds more than 3 values (max_values)"
    );
    assert!(error.hint().is_some_and(|h| h.contains("max_values")));
    // A declared length is checked whole, before anything is built.
    let mut count = ValueCount::new(10);
    assert!(count.add(usize::MAX).is_err());
    assert_eq!(count.count(), 0);
    // A limit of 0 allows nothing.
    assert!(ValueCount::new(0).add(1).is_err());
    assert!(ValueCount::new(0).add(0).is_ok());
}

#[test]
fn a_diamond_of_depth_30_is_refused_within_the_limit() {
    let data = diamond(30);
    let limits = Limits::default();
    let started = std::time::Instant::now();
    let (outcome, counted) = on_small_stack(|| {
        let mut count = ValueCount::new(limits.max_values);
        let outcome = convert(&data, &mut count);
        (outcome, count.count())
    });
    assert_eq!(counted, limits.max_values);
    let error = outcome.unwrap_err();
    assert_eq!(error.code(), ErrorCode::LimitExceeded);
    // Fails fast: a million values, not a thousand million.
    assert!(started.elapsed().as_secs() < 30);
    // A smaller diamond fits exactly: 2^11 - 1 values.
    let mut count = ValueCount::new((1 << 11) - 1);
    assert!(convert(&diamond(10), &mut count).is_ok());
    let mut count = ValueCount::new((1 << 11) - 2);
    assert!(convert(&diamond(10), &mut count).is_err());
}
