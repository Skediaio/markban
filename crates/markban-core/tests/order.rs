#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use markban_core::order::{self, Decimal};
use std::collections::BTreeSet;

fn d(s: &str) -> Decimal {
    Decimal::parse(s).unwrap()
}

fn units(x: &Decimal) -> i128 {
    x.to_scaled().unwrap()
}

#[test]
fn empty_column_gets_1000_exactly() {
    assert_eq!(order::end("x", None).unwrap().to_string(), "1000");
    assert_eq!(order::top("x", None).unwrap().to_string(), "1000");
}

#[test]
fn end_adds_1000_plus_small_offset() {
    let v = order::end("x", Some(&d("3000"))).unwrap();
    let delta = units(&v) - units(&d("3000"));
    assert!((1_000_000_000..1_100_000_000).contains(&delta), "{v}");
    assert_eq!(v.to_string().find('.'), None, "end uses whole numbers: {v}");
}

#[test]
fn top_subtracts_1000_plus_small_offset() {
    let v = order::top("x", Some(&d("1000"))).unwrap();
    let delta = units(&d("1000")) - units(&v);
    assert!((1_000_000_000..1_100_000_000).contains(&delta), "{v}");
}

#[test]
fn top_can_go_negative() {
    let v = order::top("x", Some(&d("5"))).unwrap();
    assert!(v < d("0"), "{v}");
}

#[test]
fn between_is_strictly_between_near_the_midpoint() {
    let v = order::between("x", &d("1000"), &d("2000")).unwrap();
    assert!(d("1000") < v && v < d("2000"), "{v}");
    let off = (units(&v) - units(&d("1500"))).abs();
    assert!(off <= 167_000_000, "{v}");
}

#[test]
fn between_uses_the_fewest_decimals() {
    let v = order::between("x", &d("1000"), &d("1001")).unwrap();
    assert!(d("1000") < v && v < d("1001"), "{v}");
    let s = v.to_string();
    let decimals = s.split_once('.').map_or(0, |(_, f)| f.len());
    assert!(decimals <= 2, "gap of 1 needs few decimals: {s}");
    let v = order::between("x", &d("1000"), &d("1000.000002")).unwrap();
    assert_eq!(v.to_string(), "1000.000001");
}

#[test]
fn between_without_room_is_an_error_not_a_panic() {
    // Adjacent at the neighbours' own scale (capped at 18 decimals): nothing fits.
    assert!(order::between("x", &d("1.000000000000000001"), &d("1.000000000000000002")).is_err());
    assert!(order::between("x", &d("5"), &d("5")).is_err());
    assert!(order::between("x", &d("6"), &d("5")).is_err());
    let msg = order::between("x", &d("5"), &d("5"))
        .unwrap_err()
        .to_string();
    assert!(msg.contains("before") && msg.contains("after"), "{msg}");
}

#[test]
fn unrepresentable_neighbours_are_an_error() {
    assert!(order::end("x", Some(&d(&"9".repeat(60)))).is_err());
    assert!(order::between("x", &d("1.00000000000000000001"), &d("2")).is_err());
}

#[test]
fn different_ids_get_different_orders_in_the_same_gap() {
    let values: BTreeSet<String> = (0..50)
        .map(|n| {
            order::between(&format!("card-{n}"), &d("1000"), &d("2000"))
                .unwrap()
                .to_string()
        })
        .collect();
    assert!(values.len() >= 40, "only {} distinct", values.len());
    let a = order::between("alpha", &d("1000"), &d("2000")).unwrap();
    let b = order::between("beta", &d("1000"), &d("2000")).unwrap();
    assert_ne!(a, b);
    let a = order::end("alpha", Some(&d("1000"))).unwrap();
    let b = order::end("beta", Some(&d("1000"))).unwrap();
    assert_ne!(a, b);
}

#[test]
fn allocation_is_deterministic() {
    let a = order::between("same", &d("10"), &d("20")).unwrap();
    let b = order::between("same", &d("10"), &d("20")).unwrap();
    assert_eq!(a.to_string(), b.to_string());
}

#[test]
fn decimal_display_and_scaled_roundtrip() {
    for s in [
        "0",
        "1000",
        "-12",
        "1500.25",
        "-0.5",
        "0.000001",
        "123456.789",
    ] {
        let x = d(s);
        assert_eq!(x.to_string(), s);
        assert_eq!(Decimal::from_scaled(x.to_scaled().unwrap()), x);
    }
    assert_eq!(d("-0").to_string(), "0");
}

proptest::proptest! {
    #[test]
    fn between_never_panics_and_stays_between(lo in -10_i128.pow(12)..10_i128.pow(12), gap in 0_i128..10_i128.pow(10), id in "\\PC{0,12}") {
        let (a, b) = (Decimal::from_scaled(lo), Decimal::from_scaled(lo + gap));
        match order::between(&id, &a, &b) {
            Ok(v) => proptest::prop_assert!(a < v && v < b),
            Err(_) => proptest::prop_assert!(gap < 2),
        }
    }
}

#[test]
fn end_and_top_work_with_more_than_six_decimals() {
    let x = d("1500.1234567");
    let e = order::end("x", Some(&x)).unwrap();
    assert!(e > x && e > d("2500"), "{e}");
    let t = order::top("x", Some(&x)).unwrap();
    assert!(t < x && t < d("500.2"), "{t}");
    let n = d("-1500.1234567");
    assert!(order::top("x", Some(&n)).unwrap() < n);
    assert!(order::end("x", Some(&n)).unwrap() > n);
}

#[test]
fn between_uses_the_neighbours_precision_plus_one_digit() {
    let (lo, hi) = (d("1500.1234567"), d("1500.1234569"));
    let v = order::between("x", &lo, &hi).unwrap();
    assert!(lo < v && v < hi, "{v}");
    let (lo, hi) = (d("1500.1234567"), d("1500.12345671"));
    let v = order::between("x", &lo, &hi).unwrap();
    assert!(lo < v && v < hi, "{v}");
    // Past the cap it is an error, never a panic.
    let (lo, hi) = (d("0.1234567890123456789012"), d("0.1234567890123456789014"));
    assert!(order::between("x", &lo, &hi).is_err());
    assert!(order::end("x", Some(&d(&format!("1{}", "0".repeat(60))))).is_err());
}

#[test]
fn between_works_one_decimal_beyond_a_six_decimal_neighbour() {
    // Spec section 7: one more decimal than the longest neighbour (at least 6, at most 18).
    let v = order::between("x", &d("1000"), &d("1000.000001")).unwrap();
    assert!(d("1000") < v && v < d("1000.000001"), "{v}");
    let decimals = v.to_string().split_once('.').map_or(0, |(_, f)| f.len());
    assert_eq!(decimals, 7, "{v}");
    let v = order::between("x", &d("1.123456"), &d("1.123457")).unwrap();
    assert!(d("1.123456") < v && v < d("1.123457"), "{v}");
}

#[test]
fn no_room_names_the_actual_scale() {
    let msg = order::between("x", &d("1.000000000000000001"), &d("1.000000000000000002"))
        .unwrap_err()
        .to_string();
    assert!(msg.contains("18 decimals"), "{msg}");
    let msg = order::between("x", &d("5"), &d("5"))
        .unwrap_err()
        .to_string();
    assert!(msg.contains("6 decimals"), "{msg}");
}
