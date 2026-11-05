//! Properties of the deterministic noise sources.

use vcb_core::noise::{self, Pcg32};

/// Draws `count` random argument tuples `(x, y, salt, seed)` from a fixed stream.
fn arguments(count: usize) -> Vec<(u32, u32, u32, u64)> {
    let mut rng = Pcg32::new(7, 3);
    (0..count)
        .map(|_| {
            let seed = (u64::from(rng.next_u32()) << 32) | u64::from(rng.next_u32());
            (
                rng.next_u32() % 4096,
                rng.next_u32() % 4096,
                rng.next_u32(),
                seed,
            )
        })
        .collect()
}

fn mean_and_variance(values: &[f32]) -> (f64, f64) {
    let n = values.len() as f64;
    let mean = values.iter().map(|&v| f64::from(v)).sum::<f64>() / n;
    let variance = values
        .iter()
        .map(|&v| (f64::from(v) - mean).powi(2))
        .sum::<f64>()
        / n;
    (mean, variance)
}

#[test]
fn coordinate_hash_is_a_pure_function() {
    for (x, y, salt, seed) in arguments(1000) {
        assert_eq!(
            noise::hash_u32(x, y, salt, seed),
            noise::hash_u32(x, y, salt, seed)
        );
        assert_eq!(
            noise::gauss(x, y, salt, seed).to_bits(),
            noise::gauss(x, y, salt, seed).to_bits()
        );
    }
}

#[test]
fn changing_any_argument_changes_the_hash() {
    let samples = arguments(10_000);
    let changed = |vary: &dyn Fn(u32, u32, u32, u64) -> (u32, u32, u32, u64)| {
        samples
            .iter()
            .filter(|&&(x, y, salt, seed)| {
                let (x2, y2, salt2, seed2) = vary(x, y, salt, seed);
                noise::hash_u32(x, y, salt, seed) != noise::hash_u32(x2, y2, salt2, seed2)
            })
            .count()
    };
    let minimum = samples.len() * 999 / 1000;
    assert!(changed(&|x, y, s, k| (x + 1, y, s, k)) >= minimum);
    assert!(changed(&|x, y, s, k| (x, y + 1, s, k)) >= minimum);
    assert!(changed(&|x, y, s, k| (x, y, s.wrapping_add(1), k)) >= minimum);
    assert!(changed(&|x, y, s, k| (x, y, s, k.wrapping_add(1))) >= minimum);
    assert!(changed(&|x, y, s, k| (y, x, s, k)) >= minimum * 99 / 100);
}

#[test]
fn neighbouring_photosites_are_uncorrelated() {
    let n = 20_000u32;
    let a: Vec<f64> = (0..n)
        .map(|i| f64::from(noise::signed(i % 200, i / 200, 5, 11)))
        .collect();
    let b: Vec<f64> = (0..n)
        .map(|i| f64::from(noise::signed(i % 200 + 1, i / 200, 5, 11)))
        .collect();
    let c: Vec<f64> = (0..n)
        .map(|i| f64::from(noise::signed(i % 200, i / 200 + 1, 5, 11)))
        .collect();
    let correlation = |p: &[f64], q: &[f64]| {
        p.iter().zip(q).map(|(u, v)| u * v).sum::<f64>()
            / (p.iter().map(|u| u * u).sum::<f64>() * q.iter().map(|v| v * v).sum::<f64>()).sqrt()
    };
    assert!(
        correlation(&a, &b).abs() < 0.03,
        "horizontal neighbours correlate"
    );
    assert!(
        correlation(&a, &c).abs() < 0.03,
        "vertical neighbours correlate"
    );
}

#[test]
fn unit_and_signed_stay_in_their_half_open_ranges() {
    let mut low = f32::MAX;
    let mut high = f32::MIN;
    let mut values = Vec::with_capacity(100_000);
    for (x, y, salt, seed) in arguments(100_000) {
        let u = noise::unit(x, y, salt, seed);
        assert!((0.0..1.0).contains(&u), "unit returned {u}");
        let s = noise::signed(x, y, salt, seed);
        assert!((-1.0..1.0).contains(&s), "signed returned {s}");
        low = low.min(u);
        high = high.max(u);
        values.push(u);
    }
    assert!(
        low < 1e-3 && high > 1.0 - 1e-3,
        "unit covers only [{low}, {high}]"
    );
    let (mean, variance) = mean_and_variance(&values);
    assert!((mean - 0.5).abs() < 0.01, "unit mean {mean}");
    assert!(
        (variance - 1.0 / 12.0).abs() < 0.005,
        "unit variance {variance}"
    );
}

#[test]
fn coordinate_gauss_is_standard_normal() {
    let values: Vec<f32> = (0..100_000u32)
        .map(|i| noise::gauss(i % 317, i / 317, 9, 1234))
        .collect();
    let (mean, variance) = mean_and_variance(&values);
    assert!(mean.abs() < 0.02, "gauss mean {mean}");
    assert!((variance - 1.0).abs() < 0.05, "gauss variance {variance}");
}

#[test]
fn stream_gauss_is_standard_normal() {
    let mut rng = Pcg32::new(99, 1);
    let values: Vec<f32> = (0..100_000).map(|_| rng.gauss()).collect();
    let (mean, variance) = mean_and_variance(&values);
    assert!(mean.abs() < 0.02, "gauss mean {mean}");
    assert!((variance - 1.0).abs() < 0.05, "gauss variance {variance}");
}

#[test]
fn pcg32_matches_the_reference_implementation() {
    // First outputs of `pcg32-demo` from the PCG reference C code, seeded (42, 54).
    let mut rng = Pcg32::new(42, 54);
    let expected = [
        0xa15c_02b7,
        0x7b47_f409,
        0xba1d_3330,
        0x83d2_f293,
        0xbfa4_784b,
        0xcbed_606e,
    ];
    for want in expected {
        assert_eq!(rng.next_u32(), want);
    }
}

#[test]
fn pcg32_is_reproducible_from_its_seed() {
    let draw = |seed: u64, stream: u64| {
        let mut rng = Pcg32::new(seed, stream);
        (0..1000).map(|_| rng.next_u32()).collect::<Vec<_>>()
    };
    assert_eq!(draw(5, 0), draw(5, 0));
    assert_ne!(draw(5, 0), draw(6, 0));
    assert_ne!(draw(5, 0), draw(5, 1));

    let mut rng = Pcg32::new(5, 0);
    for _ in 0..10_000 {
        let value = rng.next_f32();
        assert!((0.0..1.0).contains(&value), "next_f32 returned {value}");
    }
}

#[test]
fn stage_seeds_separate_stages_and_presets() {
    let stages = ["scene", "sensor", "readout", "afe", "bus", "isp"];
    let mut seeds: Vec<u64> = stages
        .iter()
        .map(|name| noise::stage_seed(1, noise::salt(name)))
        .collect();
    seeds.extend(
        stages
            .iter()
            .map(|name| noise::stage_seed(2, noise::salt(name))),
    );
    let mut unique = seeds.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), seeds.len());
}
