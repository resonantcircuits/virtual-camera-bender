//! The serial, leak and memory data-pin sections and the camera keys they read: they
//! parse with their defaults, round-trip, name the offending field when out of range,
//! and pass through a running simulator, which keeps its state as they come and go.

#![cfg(feature = "serde")]

use vcb_core::preset::{
    AddressFault, DataFault, Edge, FaultKind, LeakBend, MemoryBend, Mode, Resolution, SerialBend,
};
use vcb_core::{Camera, Frame, Preset, Reconfigured, Scene, Simulator};

const WIDTH: u32 = 64;
const HEIGHT: u32 = 48;

/// Frames rendered from reset in each comparison.
const FRAMES: u64 = 3;

/// The outcome of a reconfiguration that keeps every buffer and all state.
const KEPT: Reconfigured = Reconfigured::Kept {
    bus_reset: false,
    servo_reset: false,
};

fn root() -> std::path::PathBuf {
    std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn camera_text() -> String {
    std::fs::read_to_string(root().join("cameras/a520.toml")).unwrap()
}

fn camera() -> Camera {
    toml::from_str(&camera_text()).unwrap()
}

/// The shipped clean preset in `mode`, with the sensor set to the scene's size.
fn clean(mode: Mode) -> Preset {
    let text = std::fs::read_to_string(root().join("presets/a520-clean.toml")).unwrap();
    let mut preset: Preset = toml::from_str(&text).unwrap();
    preset.readout.mode = mode;
    preset.readout.resolution = Resolution::MatchInput;
    preset
}

/// A solid fault of `kind` on pin `pin`.
fn solid(pin: u8, kind: FaultKind) -> DataFault {
    DataFault {
        pin,
        kind,
        with: None,
        on_frac: None,
        switch_hz: None,
    }
}

/// A preset with the serial, leak and memory sections away from their defaults.
fn stage19_preset() -> Preset {
    let mut preset = Preset::new("a520");
    preset.bend.serial = Some(SerialBend { bit_error: 0.02 });
    preset.bend.leak = Some(LeakBend {
        edge: Edge::Bottom,
        radiance: 1.5,
        spread_frac: 0.6,
        colour: [1.0, 0.45, 0.3],
        nir: 2.0,
    });
    preset.bend.memory = Some(MemoryBend {
        faults: vec![AddressFault {
            pin: 3,
            kind: FaultKind::High,
            with: None,
            on_frac: None,
            switch_hz: None,
        }],
        data_faults: vec![
            solid(9, FaultKind::Low),
            DataFault {
                with: Some(6),
                on_frac: Some(0.3),
                switch_hz: Some(4.0),
                ..solid(5, FaultKind::Bridge)
            },
            DataFault {
                on_frac: Some(1.0),
                switch_hz: Some(0.0),
                ..solid(3, FaultKind::High)
            },
        ],
        temperature_c: Some(110.0),
    });
    preset
}

/// Copies one section from the second preset into the first.
type CopySection = (&'static str, fn(&mut Preset, &Preset));

#[test]
fn the_new_sections_round_trip_through_toml() {
    let preset = stage19_preset();
    preset.validate().unwrap();
    let text = toml::to_string(&preset).unwrap();
    for needle in [
        "[bend.serial]",
        "[bend.leak]",
        "edge = \"bottom\"",
        "[[bend.memory.faults]]",
        "[[bend.memory.data_faults]]",
        "kind = \"bridge\"",
        "temperature_c = 110",
    ] {
        assert!(text.contains(needle), "missing {needle} in\n{text}");
    }
    let back: Preset = toml::from_str(&text).unwrap();
    assert_eq!(back, preset);

    // Each section on its own; the absent sections stay absent.
    let sections: [CopySection; 3] = [
        ("serial", |p, from| p.bend.serial = from.bend.serial),
        ("leak", |p, from| p.bend.leak = from.bend.leak),
        ("memory", |p, from| {
            p.bend.memory.clone_from(&from.bend.memory);
        }),
    ];
    for (name, set) in sections {
        let mut alone = Preset::new("a520");
        set(&mut alone, &preset);
        let text = toml::to_string(&alone).unwrap();
        for (other, _) in sections {
            let written = text.contains(&format!("bend.{other}"));
            assert_eq!(written, other == name, "{name} alone:\n{text}");
        }
        let back: Preset = toml::from_str(&text).unwrap();
        assert_eq!(back, alone, "{name} alone");
    }
    let plain = toml::to_string(&Preset::new("a520")).unwrap();
    for (name, _) in sections {
        assert!(!plain.contains(&format!("bend.{name}")), "{plain}");
    }
}

#[test]
fn a_memory_section_without_the_new_keys_writes_none_of_them() {
    let mut preset = Preset::new("a520");
    preset.bend.memory = Some(MemoryBend::default());
    let text = toml::to_string(&preset).unwrap();
    for key in ["data_faults", "temperature_c"] {
        assert!(!text.contains(key), "{key} in\n{text}");
    }
    let back: Preset = toml::from_str(&text).unwrap();
    assert_eq!(back, preset);
}

#[test]
fn the_new_sections_parse_with_their_defaults() {
    let text = "camera = \"a520\"\n[bend.serial]\n[bend.leak]\n[bend.memory]\n";
    let preset: Preset = toml::from_str(text).unwrap();
    preset.validate().unwrap();
    assert_eq!(preset.bend.serial, Some(SerialBend::default()));
    assert_eq!(preset.bend.leak, Some(LeakBend::default()));
    assert_eq!(preset.bend.memory, Some(MemoryBend::default()));

    // A section on its own is a bent contact: it flips some bits, but few.
    let serial = SerialBend::default();
    assert!(
        serial.bit_error > 0.0 && serial.bit_error < 0.01,
        "{serial:?}"
    );
    // A daylight leak from the left: white light, daylight's near infrared, visible.
    let leak = LeakBend::default();
    assert_eq!(leak.edge, Edge::Left);
    assert!(leak.radiance > 0.0, "{leak:?}");
    assert!(
        leak.colour.iter().all(|&c| (c - 1.0).abs() < f64::EPSILON),
        "{leak:?}"
    );
    assert!((leak.nir - 1.0).abs() < f64::EPSILON, "{leak:?}");
    // The memory's own temperature, no data fault.
    let memory = MemoryBend::default();
    assert!(memory.data_faults.is_empty() && memory.temperature_c.is_none());

    let text = r#"
        camera = "a520"
        [bend.leak]
        edge = "top"
        [bend.memory]
        temperature_c = 120.0
        [[bend.memory.data_faults]]
        pin = 9
        kind = "low"
        [[bend.memory.data_faults]]
        pin = 5
        kind = "bridge"
        with = 4
    "#;
    let preset: Preset = toml::from_str(text).unwrap();
    preset.validate().unwrap();
    let leak = preset.bend.leak.unwrap();
    assert_eq!(leak.edge, Edge::Top);
    assert_eq!(
        LeakBend {
            edge: Edge::Left,
            ..leak
        },
        LeakBend::default(),
        "the other keys keep their defaults"
    );
    let memory = preset.bend.memory.unwrap();
    assert_eq!(memory.faults, Vec::<AddressFault>::new());
    assert_eq!(memory.temperature_c, Some(120.0));
    assert_eq!(memory.data_faults[0], solid(9, FaultKind::Low));
    assert_eq!(memory.data_faults[1].with, Some(4));
}

#[test]
fn edges_display_as_they_are_written() {
    for edge in [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom] {
        let leak = LeakBend {
            edge,
            ..LeakBend::default()
        };
        let mut preset = Preset::new("a520");
        preset.bend.leak = Some(leak);
        let text = toml::to_string(&preset).unwrap();
        assert!(text.contains(&format!("edge = \"{edge}\"")), "{text}");
    }
}

#[test]
fn the_new_sections_reject_unknown_keys_and_values() {
    let cases = [
        ("[bend.serial]\nbit_errors = 0.1", "bit_errors"),
        ("[bend.leak]\ncolor = [1.0, 1.0, 1.0]", "color"),
        ("[bend.leak]\nedge = \"middle\"", "middle"),
        ("[bend.leak]\ncolour = [1.0, 1.0]", "colour"),
        ("[bend.memory]\ntemperature = 50.0", "temperature"),
        ("[bend.memory]\ndata_fault = []", "data_fault"),
        ("[[bend.memory.data_faults]]\nkind = \"low\"", "pin"),
        ("[[bend.memory.data_faults]]\npin = 1", "kind"),
        (
            "[[bend.memory.data_faults]]\npin = 1\nkind = \"stuck\"",
            "stuck",
        ),
        (
            "[[bend.memory.data_faults]]\npin = 1\nkind = \"low\"\nwidth = 2",
            "width",
        ),
    ];
    for (section, needle) in cases {
        let text = format!("camera = \"a520\"\n{section}");
        let error = toml::from_str::<Preset>(&text).unwrap_err();
        assert!(error.to_string().contains(needle), "{section}: {error}");
    }
}

/// Breaks one value of a preset.
type BreakPreset = fn(&mut Preset);

fn serial_of(preset: &mut Preset) -> &mut SerialBend {
    preset.bend.serial.as_mut().unwrap()
}

fn leak_of(preset: &mut Preset) -> &mut LeakBend {
    preset.bend.leak.as_mut().unwrap()
}

fn memory_of(preset: &mut Preset) -> &mut MemoryBend {
    preset.bend.memory.as_mut().unwrap()
}

fn data_of(preset: &mut Preset) -> &mut Vec<DataFault> {
    &mut memory_of(preset).data_faults
}

/// Asserts that each broken preset fails validation with an error naming its field.
fn assert_names(cases: &[(BreakPreset, &str)]) {
    for (case, (breaks, field)) in cases.iter().enumerate() {
        let mut preset = stage19_preset();
        breaks(&mut preset);
        let error = preset.validate().unwrap_err();
        assert!(error.is_config(), "case {case}: {error}");
        assert!(
            error.to_string().contains(field),
            "case {case}: \"{error}\" does not name {field}"
        );
    }
}

#[test]
fn serial_leak_and_temperature_validation_names_the_offending_field() {
    assert_names(&[
        (|p| serial_of(p).bit_error = -0.01, "bend.serial.bit_error"),
        (|p| serial_of(p).bit_error = 1.5, "bend.serial.bit_error"),
        (
            |p| serial_of(p).bit_error = f64::NAN,
            "bend.serial.bit_error",
        ),
        (|p| leak_of(p).radiance = -1.0, "bend.leak.radiance"),
        (
            |p| leak_of(p).radiance = f64::INFINITY,
            "bend.leak.radiance",
        ),
        (|p| leak_of(p).spread_frac = 0.0, "bend.leak.spread_frac"),
        (|p| leak_of(p).spread_frac = -0.2, "bend.leak.spread_frac"),
        (|p| leak_of(p).spread_frac = 2.5, "bend.leak.spread_frac"),
        (
            |p| leak_of(p).spread_frac = f64::NAN,
            "bend.leak.spread_frac",
        ),
        (|p| leak_of(p).colour[0] = -0.1, "bend.leak.colour (r)"),
        (|p| leak_of(p).colour[2] = f64::NAN, "bend.leak.colour (b)"),
        (|p| leak_of(p).nir = -1.0, "bend.leak.nir"),
        (|p| leak_of(p).nir = f64::INFINITY, "bend.leak.nir"),
        (
            |p| memory_of(p).temperature_c = Some(200.0),
            "bend.memory.temperature_c",
        ),
        (
            |p| memory_of(p).temperature_c = Some(-60.0),
            "bend.memory.temperature_c",
        ),
        (
            |p| memory_of(p).temperature_c = Some(f64::NAN),
            "bend.memory.temperature_c",
        ),
    ]);
}

#[test]
fn data_fault_validation_names_the_offending_field() {
    assert_names(&[
        // A bridge names its partner, another pin; a stuck pin has none.
        (
            |p| data_of(p)[1].with = None,
            "bend.memory.data_faults[1].with",
        ),
        (
            |p| data_of(p)[1].with = Some(5),
            "bend.memory.data_faults[1].with",
        ),
        (
            |p| data_of(p)[0].with = Some(8),
            "bend.memory.data_faults[0].with",
        ),
        // A marginal contact sets both of its values, each in range.
        (
            |p| data_of(p)[1].on_frac = Some(0.0),
            "bend.memory.data_faults[1].on_frac",
        ),
        (
            |p| data_of(p)[1].on_frac = Some(1.5),
            "bend.memory.data_faults[1].on_frac",
        ),
        (
            |p| data_of(p)[1].switch_hz = Some(-1.0),
            "bend.memory.data_faults[1].switch_hz",
        ),
        (
            |p| data_of(p)[1].switch_hz = Some(f64::NAN),
            "bend.memory.data_faults[1].switch_hz",
        ),
        (
            |p| data_of(p)[1].switch_hz = None,
            "bend.memory.data_faults[1].on_frac",
        ),
        (
            |p| data_of(p)[1].on_frac = None,
            "bend.memory.data_faults[1].switch_hz",
        ),
        // Each data pin carries one fault, as `pin` or as a bridge's `with`.
        (
            |p| data_of(p).push(solid(9, FaultKind::High)),
            "bend.memory.data_faults[3].pin",
        ),
        (
            |p| data_of(p).push(solid(6, FaultKind::Low)),
            "bend.memory.data_faults[3].pin",
        ),
        (
            |p| data_of(p)[1].with = Some(9),
            "bend.memory.data_faults[1].with",
        ),
        (
            |p| {
                data_of(p).push(DataFault {
                    with: Some(5),
                    ..solid(7, FaultKind::Bridge)
                });
            },
            "bend.memory.data_faults[3].with",
        ),
        // The address faults keep their own rules beside the data faults.
        (
            |p| memory_of(p).faults[0].with = Some(4),
            "bend.memory.faults[0].with",
        ),
        (
            |p| {
                memory_of(p).faults.push(AddressFault {
                    pin: 3,
                    kind: FaultKind::Low,
                    with: None,
                    on_frac: None,
                    switch_hz: None,
                });
            },
            "bend.memory.faults[1].pin",
        ),
    ]);
}

#[test]
fn a_repeated_data_pin_is_named_as_a_data_pin() {
    let mut preset = stage19_preset();
    data_of(&mut preset).push(solid(9, FaultKind::High));
    let error = preset.validate().unwrap_err();
    assert!(error.to_string().contains("pin d9"), "{error}");
    let mut preset = stage19_preset();
    memory_of(&mut preset).faults.push(AddressFault {
        pin: 3,
        kind: FaultKind::Low,
        with: None,
        on_frac: None,
        switch_hz: None,
    });
    let error = preset.validate().unwrap_err();
    assert!(error.to_string().contains("pin a3"), "{error}");
}

#[test]
fn the_ranges_hold_their_ends_and_address_and_data_pins_are_apart() {
    let ends: [BreakPreset; 8] = [
        |p| serial_of(p).bit_error = 0.0,
        |p| serial_of(p).bit_error = 1.0,
        |p| leak_of(p).radiance = 0.0,
        |p| leak_of(p).spread_frac = 2.0,
        |p| leak_of(p).colour = [0.0; 3],
        |p| leak_of(p).nir = 0.0,
        |p| memory_of(p).temperature_c = Some(-40.0),
        |p| memory_of(p).temperature_c = Some(150.0),
    ];
    for (case, set) in ends.iter().enumerate() {
        let mut preset = stage19_preset();
        set(&mut preset);
        preset
            .validate()
            .unwrap_or_else(|error| panic!("case {case}: {error}"));
    }
    // Address pin a3 and data pin d3 are different pins and may both be faulted.
    let preset = stage19_preset();
    let memory = preset.bend.memory.as_ref().unwrap();
    assert!(
        memory
            .faults
            .iter()
            .any(|a| memory.data_faults.iter().any(|d| d.pin == a.pin)),
        "the preset should fault one number on both buses"
    );
    preset.validate().unwrap();
}

#[test]
fn the_camera_file_holds_the_new_keys() {
    let camera = camera();
    camera.validate().unwrap();
    let back: Camera = toml::from_str(&toml::to_string(&camera).unwrap()).unwrap();
    assert_eq!(back, camera);

    // A word holds the processor's sample.
    assert!(camera.memory.data_pins >= camera.adc.dsp_bits);
    // At the reference temperature no cell's retention reaches the refresh period
    // within seven sigma of the log-normal spread, so the stock SDRAM keeps its bits.
    let memory = &camera.memory;
    let weakest_s = memory.retention_s * (-7.0 * memory.retention_spread).exp();
    assert!(
        weakest_s > memory.refresh_ms / 1000.0,
        "a cell at seven sigma holds {weakest_s} s against a {} ms refresh",
        memory.refresh_ms
    );
    // The camera's nominal register contents fit the registers: the VGA's codes span
    // its gain, the clamp's its pedestal, and the PxGA gains lie in their range.
    let afe = &camera.afe;
    let vga_top_db = afe.vga_min_db + afe.vga_db_per_code * f64::from((1u32 << afe.vga_bits) - 1);
    assert!(
        (afe.vga_min_db..=vga_top_db).contains(&afe.pga_gain_db),
        "pga_gain_db {} outside the VGA's {} to {vga_top_db} dB",
        afe.pga_gain_db,
        afe.vga_min_db
    );
    assert!(u32::from(camera.adc.black_level) < 1u32 << afe.clamp_bits);
    assert!(
        afe.pxga_gain_db
            .iter()
            .all(|gain| (0.0..=afe.pxga_max_db).contains(gain))
    );
    let widest = afe.vga_bits.max(afe.pxga_bits).max(afe.clamp_bits);
    assert!(afe.serial_data_bits >= widest);
}

#[test]
fn the_new_camera_keys_are_required() {
    let file: toml::Table = toml::from_str(&camera_text()).unwrap();
    for (section, key) in [
        ("memory", "data_pins"),
        ("memory", "reference_temp_c"),
        ("memory", "retention_s"),
        ("memory", "retention_halving_c"),
        ("memory", "retention_spread"),
        ("memory", "refresh_ms"),
        ("afe", "serial_data_bits"),
        ("afe", "vga_bits"),
        ("afe", "vga_min_db"),
        ("afe", "vga_db_per_code"),
        ("afe", "pxga_bits"),
        ("afe", "pxga_max_db"),
        ("afe", "clamp_bits"),
        ("afe", "pxga_gain_db"),
    ] {
        let mut without = file.clone();
        without
            .get_mut(section)
            .and_then(toml::Value::as_table_mut)
            .unwrap()
            .remove(key)
            .unwrap_or_else(|| panic!("the camera file has no {section}.{key}"));
        let error = toml::from_str::<Camera>(&toml::to_string(&without).unwrap()).unwrap_err();
        assert!(error.to_string().contains(key), "{key}: {error}");
    }
}

/// Breaks one value of a camera.
type BreakCamera = fn(&mut Camera);

#[test]
fn the_new_camera_keys_are_validated() {
    let cases: [(BreakCamera, &str); 26] = [
        (|c| c.memory.data_pins = 0, "memory.data_pins"),
        (|c| c.memory.data_pins = 17, "memory.data_pins"),
        // A word narrower than the processor's sample cannot hold it.
        (
            |c| c.memory.data_pins = c.adc.dsp_bits - 1,
            "memory.data_pins",
        ),
        (
            |c| c.memory.reference_temp_c = 200.0,
            "memory.reference_temp_c",
        ),
        (
            |c| c.memory.reference_temp_c = f64::NAN,
            "memory.reference_temp_c",
        ),
        (|c| c.memory.retention_s = 0.0, "memory.retention_s"),
        (|c| c.memory.retention_s = f64::NAN, "memory.retention_s"),
        (
            |c| c.memory.retention_halving_c = 0.0,
            "memory.retention_halving_c",
        ),
        (
            |c| c.memory.retention_halving_c = -10.0,
            "memory.retention_halving_c",
        ),
        (
            |c| c.memory.retention_spread = -0.1,
            "memory.retention_spread",
        ),
        (
            |c| c.memory.retention_spread = f64::INFINITY,
            "memory.retention_spread",
        ),
        (|c| c.memory.refresh_ms = 0.0, "memory.refresh_ms"),
        (|c| c.afe.serial_data_bits = 0, "afe.serial_data_bits"),
        (|c| c.afe.serial_data_bits = 33, "afe.serial_data_bits"),
        // The data field holds the widest register.
        (
            |c| c.afe.serial_data_bits = c.afe.vga_bits - 1,
            "afe.serial_data_bits",
        ),
        (|c| c.afe.vga_bits = 0, "afe.vga_bits"),
        (|c| c.afe.vga_bits = 17, "afe.vga_bits"),
        (|c| c.afe.vga_min_db = f64::NAN, "afe.vga_min_db"),
        (|c| c.afe.vga_db_per_code = 0.0, "afe.vga_db_per_code"),
        (|c| c.afe.pxga_bits = 0, "afe.pxga_bits"),
        (|c| c.afe.pxga_max_db = 0.0, "afe.pxga_max_db"),
        (|c| c.afe.clamp_bits = 17, "afe.clamp_bits"),
        (|c| c.afe.clamp_bits = 0, "afe.clamp_bits"),
        (|c| c.afe.pxga_gain_db[0] = -1.0, "afe.pxga_gain_db (r)"),
        (
            |c| c.afe.pxga_gain_db[2] = c.afe.pxga_max_db + 0.5,
            "afe.pxga_gain_db (gb)",
        ),
        (|c| c.afe.pxga_gain_db[3] = f64::NAN, "afe.pxga_gain_db (b)"),
    ];
    for (case, (breaks, field)) in cases.iter().enumerate() {
        let mut camera = camera();
        breaks(&mut camera);
        let error = camera.validate().unwrap_err();
        assert!(error.is_config(), "case {case}: {error}");
        assert!(
            error.to_string().contains(field),
            "case {case}: \"{error}\" does not name {field}"
        );
    }

    // The ends of the ranges hold.
    let ends: [BreakCamera; 4] = [
        |c| c.memory.data_pins = c.adc.dsp_bits,
        |c| c.memory.retention_spread = 0.0,
        |c| c.afe.pxga_gain_db = [c.afe.pxga_max_db; 4],
        |c| c.afe.serial_data_bits = c.afe.vga_bits,
    ];
    for (case, set) in ends.iter().enumerate() {
        let mut camera = camera();
        set(&mut camera);
        camera
            .validate()
            .unwrap_or_else(|error| panic!("case {case}: {error}"));
    }
}

fn build(preset: Preset) -> Simulator {
    Simulator::builder((camera(), preset))
        .input_size(WIDTH, HEIGHT)
        .build()
        .unwrap()
}

/// A gradient with saturated foliage green, sky blue and warm red.
fn scene() -> Scene {
    let rgb: Vec<u8> = (0..HEIGHT)
        .flat_map(|y| {
            (0..WIDTH).flat_map(move |x| {
                let v = ((x * 255 / WIDTH + y * 96 / HEIGHT) % 256) as u8;
                match (x * 3 / WIDTH, y * 2 / HEIGHT) {
                    (0, 0) => [60, 140, 40],
                    (1, 0) => [90, 150, 230],
                    (2, 0) => [220, 70, 40],
                    _ => [v, v / 2 + 60, 255 - v],
                }
            })
        })
        .collect();
    Scene::from_srgb8(WIDTH, HEIGHT, &rgb)
}

/// Frames `0..FRAMES` in order.
fn render(sim: &mut Simulator, scene: &Scene) -> Vec<Frame> {
    (0..FRAMES)
        .map(|index| sim.frame(scene, index).clone())
        .collect()
}

/// Sets one of the new sections of a preset.
type Section = (&'static str, fn(&mut Preset));

/// Each new section bent: a noisy serial contact, a leak, data pins and a heat gun.
const BENT: [Section; 4] = [
    ("serial", |p| {
        p.bend.serial = Some(SerialBend { bit_error: 0.05 });
    }),
    ("leak", |p| p.bend.leak = Some(LeakBend::default())),
    ("memory data faults", |p| {
        p.bend.memory = Some(MemoryBend {
            data_faults: vec![solid(9, FaultKind::Low), solid(5, FaultKind::High)],
            ..MemoryBend::default()
        });
    }),
    ("memory temperature", |p| {
        p.bend.memory = Some(MemoryBend {
            temperature_c: Some(120.0),
            ..MemoryBend::default()
        });
    }),
];

/// The stages do not read the new sections yet: each, and all together, render the
/// clean camera's bytes.
#[test]
fn a_preset_with_every_new_section_resolves_and_renders_the_camera() {
    let scene = scene();
    for mode in [Mode::Still, Mode::Movie] {
        let plain = clean(mode);
        let expected = render(&mut build(plain.clone()), &scene);
        for (name, set) in BENT {
            let mut alone = plain.clone();
            set(&mut alone);
            assert_eq!(
                render(&mut build(alone), &scene),
                expected,
                "{mode}: the {name} section changed the frames"
            );
        }
        // Every new key at once, without the address fault the stages do read.
        let bent = stage19_preset();
        let mut every = plain.clone();
        every.bend.serial = bent.bend.serial;
        every.bend.leak = bent.bend.leak;
        every.bend.memory = bent.bend.memory.map(|memory| MemoryBend {
            faults: Vec::new(),
            ..memory
        });
        let mut sim = build(every);
        assert_eq!(
            render(&mut sim, &scene),
            expected,
            "{mode}: the new sections together changed the frames"
        );
    }
}

#[test]
fn adding_turning_or_removing_a_new_section_keeps_the_running_camera() {
    let scene = scene();
    for mode in [Mode::Still, Mode::Movie] {
        for (name, set) in BENT {
            let plain = clean(mode);
            let mut sim = build(plain.clone());
            render(&mut sim, &scene);
            let mut with = plain.clone();
            set(&mut with);
            assert_eq!(
                sim.set_preset(with.clone()).unwrap(),
                KEPT,
                "{mode}: adding {name}"
            );
            assert_eq!(sim.preset(), &with);
            sim.frame(&scene, FRAMES);
            assert_eq!(
                sim.set_preset(plain).unwrap(),
                KEPT,
                "{mode}: removing {name}"
            );
            sim.frame(&scene, FRAMES + 1);
        }
    }

    let mut bent = clean(Mode::Movie);
    for (_, set) in BENT {
        set(&mut bent);
    }
    let mut sim = build(bent.clone());
    render(&mut sim, &scene);
    let turns: [Section; 4] = [
        ("the serial error rate", |p| {
            p.bend.serial.as_mut().unwrap().bit_error = 0.2;
        }),
        ("the leak's edge and radiance", |p| {
            let leak = p.bend.leak.as_mut().unwrap();
            leak.edge = Edge::Right;
            leak.radiance = 3.0;
        }),
        ("the data faults", |p| {
            p.bend
                .memory
                .as_mut()
                .unwrap()
                .data_faults
                .push(solid(2, FaultKind::High));
        }),
        ("the SDRAM's temperature", |p| {
            p.bend.memory.as_mut().unwrap().temperature_c = Some(90.0);
        }),
    ];
    for (index, (knob, turn)) in (FRAMES..).zip(turns) {
        turn(&mut bent);
        assert_eq!(
            sim.set_preset(bent.clone()).unwrap(),
            KEPT,
            "turning {knob}"
        );
        sim.frame(&scene, index);
    }
}
