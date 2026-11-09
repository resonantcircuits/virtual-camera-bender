//! The data-pin grid, the retention block and the compressed buffer's switch of the memory
//! panel, the serial link in the
//! AFE panel and the light leak in the sensor panel write full sections that survive
//! TOML, keep their values while switched off, hold the preset's rules, show their
//! readouts from the camera's values, light their stages on the strip, and reach the
//! running camera as kept state.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use vcb_app::panels::afe::{self, SerialPanel};
use vcb_app::panels::memory::{self, MemoryPanel, PinGrid, PinSetting};
use vcb_app::panels::sensor::{self, LeakPanel};
use vcb_app::panels::strip::Stage;
use vcb_app::panels::{PanelId, Panels};
use vcb_app::presets;
use vcb_app::render::{Job, Renderer};
use vcb_core::preset::{
    AddressFault, DataFault, Edge, FaultKind, LeakBend, MemoryBend, RailBend, Resolution,
    SerialBend,
};
use vcb_core::{Camera, Preset, Reconfigured, Scene, Simulator};

/// The A520's bendable address pins, `a0` to `a11`.
const ADDRESS_PINS: u8 = 12;

fn root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn camera() -> Camera {
    presets::load_camera(&root().join("cameras"), "a520").expect("the A520 camera file")
}

/// The A520's data pins, `d0` to `d15`.
fn data_pins() -> u8 {
    camera().memory.data_pins
}

/// Writes `preset` as TOML and reads it back, checking that nothing changed.
fn through_toml(preset: &Preset) -> Preset {
    let text = presets::to_toml(preset).expect("the preset serialises");
    let parsed = presets::from_toml(&text).expect("the written preset parses and validates");
    assert_eq!(&parsed, preset, "the preset survives TOML:\n{text}");
    parsed
}

/// A preset with `apply` written into it, through TOML.
fn written(apply: impl FnOnce(&mut Preset)) -> Preset {
    let mut preset = Preset::new("a520");
    preset.seed = 5;
    apply(&mut preset);
    through_toml(&preset)
}

/// A red leak from the top, wide, with plenty of infrared.
fn leak_panel() -> LeakPanel {
    LeakPanel {
        enabled: true,
        bend: LeakBend {
            edge: Edge::Top,
            radiance: 1.5,
            spread_frac: 0.45,
            colour: [1.2, 0.4, 0.2],
            nir: 2.5,
        },
    }
}

/// A noisy contact flipping four bits in a thousand.
fn serial_panel() -> SerialPanel {
    SerialPanel {
        enabled: true,
        bend: SerialBend { bit_error: 0.004 },
    }
}

/// A memory panel with the default address fault, every kind of data fault and a heat gun.
fn memory_panel() -> MemoryPanel {
    let count = data_pins();
    let mut panel = MemoryPanel {
        enabled: true,
        temperature_c: Some(110.0),
        ..MemoryPanel::default()
    };
    let data = &mut panel.data;
    assert!(data.set_kind(9, Some(FaultKind::High), count));
    assert!(data.set_kind(3, Some(FaultKind::Bridge), count));
    assert!(data.set_partner(3, 6, count));
    assert!(data.set_marginal(3, true));
    assert!(data.set_kind(12, Some(FaultKind::Low), count));
    data.pins[1].on_frac = 0.4;
    data.pins[1].switch_hz = 20.0;
    panel
}

/// Every pin a list of faults names, its own and a bridge's other end, in order.
fn named(faults: &[DataFault]) -> Vec<u8> {
    faults
        .iter()
        .flat_map(|fault| std::iter::once(fault.pin).chain(fault.with))
        .collect()
}

#[test]
fn each_block_round_trips_a_full_section_through_toml() {
    let panel = leak_panel();
    let preset = written(|preset| panel.apply(preset));
    assert_eq!(preset.bend.leak, Some(panel.bend));
    assert_eq!(LeakPanel::from_preset(&preset), panel);
    let text = presets::to_toml(&preset).expect("serialises");
    assert!(
        text.contains("[bend.leak]") && text.contains("edge = \"top\""),
        "the leak names its edge as the preset does:\n{text}"
    );

    let panel = serial_panel();
    let preset = written(|preset| panel.apply(preset));
    assert_eq!(preset.bend.serial, Some(panel.bend));
    assert_eq!(SerialPanel::from_preset(&preset), panel);

    let panel = memory_panel();
    let preset = written(|preset| panel.apply(preset));
    let bend = preset.bend.memory.as_ref().expect("the memory bend");
    assert_eq!(bend.temperature_c, Some(110.0));
    assert_eq!(
        bend.faults
            .iter()
            .map(|fault| fault.pin)
            .collect::<Vec<_>>(),
        [memory::DEFAULT_PIN],
        "the address grid keeps its default fault"
    );
    assert_eq!(
        bend.data_faults,
        [
            DataFault {
                pin: 9,
                kind: FaultKind::High,
                with: None,
                on_frac: None,
                switch_hz: None,
            },
            DataFault {
                pin: 3,
                kind: FaultKind::Bridge,
                with: Some(6),
                on_frac: Some(0.4),
                switch_hz: Some(20.0),
            },
            DataFault {
                pin: 12,
                kind: FaultKind::Low,
                with: None,
                on_frac: None,
                switch_hz: None,
            },
        ],
        "in the order the pins were set, the marginal contact with its values"
    );
    let mut loaded = panel.clone();
    loaded.load(&preset);
    assert_eq!(loaded, panel, "the panel reads back its own preset");
    assert_eq!(MemoryPanel::from_preset(&preset).bend(), panel.bend());

    let stock = MemoryPanel {
        enabled: true,
        ..MemoryPanel::default()
    };
    let preset = written(|preset| stock.apply(preset));
    let text = presets::to_toml(&preset).expect("serialises");
    assert!(
        !text.contains("temperature_c") && !text.contains("data_faults"),
        "the camera's temperature and sound data pins are not written:\n{text}"
    );
    assert_eq!(MemoryPanel::from_preset(&preset), stock);

    let fresh = LeakPanel {
        enabled: true,
        ..LeakPanel::default()
    };
    assert_eq!(
        fresh.bend(),
        Some(LeakBend::default()),
        "a fresh leak is the section's daylight leak"
    );
    let fresh = SerialPanel {
        enabled: true,
        ..SerialPanel::default()
    };
    assert_eq!(
        fresh.bend(),
        Some(SerialBend::default()),
        "a fresh serial link flips the section's default share of bits"
    );
}

#[test]
fn a_switched_off_block_removes_its_section_and_restores_the_same_values() {
    fn check<P: Clone + PartialEq + std::fmt::Debug>(
        name: &str,
        mut panel: P,
        enabled: fn(&mut P) -> &mut bool,
        apply: fn(&P, &mut Preset),
        load: fn(&mut P, &Preset),
        section: fn(&Preset) -> bool,
    ) {
        let on = written(|preset| apply(&panel, preset));
        assert!(section(&on), "{name}: on writes the section");
        *enabled(&mut panel) = false;
        let off = written(|preset| apply(&panel, preset));
        assert!(!section(&off), "{name}: off removes the section");
        assert_eq!(
            off,
            Preset {
                seed: 5,
                ..Preset::new("a520")
            }
        );

        let before = panel.clone();
        load(&mut panel, &off);
        assert_eq!(
            panel, before,
            "{name}: loading a preset without the bend keeps the values"
        );
        *enabled(&mut panel) = true;
        let again = written(|preset| apply(&panel, preset));
        assert_eq!(again, on, "{name}: on again restores the same section");
    }
    check(
        "light leak",
        leak_panel(),
        |panel| &mut panel.enabled,
        LeakPanel::apply,
        LeakPanel::load,
        |preset| preset.bend.leak.is_some(),
    );
    check(
        "serial link",
        serial_panel(),
        |panel| &mut panel.enabled,
        SerialPanel::apply,
        SerialPanel::load,
        |preset| preset.bend.serial.is_some(),
    );
    check(
        "memory",
        memory_panel(),
        |panel| &mut panel.enabled,
        MemoryPanel::apply,
        MemoryPanel::load,
        |preset| preset.bend.memory.is_some(),
    );

    let panels = Panels {
        leak: leak_panel(),
        serial: serial_panel(),
        memory: memory_panel(),
        ..Panels::default()
    };
    let mut preset = Preset::new("a520");
    panels.apply(&mut preset);
    let bend = &preset.bend;
    assert!(
        bend.leak.is_some() && bend.serial.is_some() && bend.memory.is_some(),
        "the panels write the three sections"
    );
    assert!(
        bend.sensor.is_none() && bend.afe.is_none() && bend.reset.is_none(),
        "the leak's and the serial link's switches are their own, apart from the blocks beside them"
    );
    assert_eq!(Panels::from_preset(&through_toml(&preset)), panels);
}

#[test]
fn an_address_edit_keeps_the_data_faults_and_the_temperature_of_a_loaded_preset() {
    let mut preset = Preset::new("a520");
    preset.bend.memory = Some(MemoryBend {
        faults: vec![AddressFault {
            pin: 10,
            kind: FaultKind::High,
            with: None,
            on_frac: None,
            switch_hz: None,
        }],
        data_faults: vec![
            DataFault {
                pin: 9,
                kind: FaultKind::High,
                with: None,
                on_frac: None,
                switch_hz: None,
            },
            DataFault {
                pin: 2,
                kind: FaultKind::Bridge,
                with: Some(7),
                on_frac: Some(0.25),
                switch_hz: Some(3.0),
            },
        ],
        temperature_c: Some(105.0),
        reaches_jpeg: true,
    });
    let preset = through_toml(&preset);
    let loaded = preset.bend.memory.clone().expect("the memory bend");

    let mut untouched = preset.clone();
    Panels::from_preset(&preset).apply(&mut untouched);
    assert_eq!(untouched, preset, "the panels write back what they loaded");

    let mut panels = Panels::from_preset(&preset);
    assert!(
        panels
            .memory
            .address
            .set_kind(3, Some(FaultKind::Low), ADDRESS_PINS)
    );
    let mut edited = preset.clone();
    panels.apply(&mut edited);
    let bend = through_toml(&edited).bend.memory.expect("the memory bend");
    assert_eq!(
        bend.faults
            .iter()
            .map(|fault| fault.pin)
            .collect::<Vec<_>>(),
        [10, 3],
        "the edit lands"
    );
    assert_eq!(
        bend.data_faults, loaded.data_faults,
        "the data faults survive an edit of an address pin"
    );
    assert_eq!(
        bend.temperature_c, loaded.temperature_c,
        "the temperature survives an edit of an address pin"
    );
    assert!(
        bend.reaches_jpeg,
        "the compressed buffer stays in the bent chip through an edit of an address pin"
    );

    assert!(panels.memory.data.set_kind(2, None, data_pins()));
    panels.memory.temperature_c = None;
    panels.apply(&mut edited);
    let bend = through_toml(&edited).bend.memory.expect("the memory bend");
    assert_eq!(
        named(&bend.data_faults),
        [9],
        "d2's bridge is gone, d7 with it"
    );
    assert_eq!(bend.temperature_c, None, "back at the camera's temperature");
    assert_eq!(
        bend.faults.len(),
        2,
        "the address faults survive edits of the data pins"
    );
}

#[test]
fn the_compressed_buffer_switch_writes_reaches_jpeg_and_reads_it_back() {
    // Untouched by default, and then not written.
    let panel = memory_panel();
    assert!(
        !panel.reaches_jpeg,
        "the compressed buffer starts out of reach"
    );
    let preset = written(|preset| panel.apply(preset));
    assert!(!preset.bend.memory.as_ref().expect("the bend").reaches_jpeg);
    let text = presets::to_toml(&preset).expect("serialises");
    assert!(!text.contains("reaches_jpeg"), "{text}");

    // Thrown: written, read back through TOML, and loaded with the rest of the section.
    let mut thrown = memory_panel();
    thrown.reaches_jpeg = true;
    let preset = written(|preset| thrown.apply(preset));
    let text = presets::to_toml(&preset).expect("serialises");
    assert!(text.contains("reaches_jpeg = true"), "{text}");
    let bend = preset.bend.memory.as_ref().expect("the bend");
    assert!(bend.reaches_jpeg);
    assert_eq!(Some(bend), thrown.bend().as_ref());
    assert_eq!(MemoryPanel::from_preset(&preset), thrown);
    let mut loaded = MemoryPanel::default();
    loaded.load(&preset);
    assert!(loaded.reaches_jpeg, "load carries the switch");
    assert_eq!(
        loaded.bend(),
        thrown.bend(),
        "and the faults and heat beside it"
    );

    // A preset without it puts the switch back.
    let mut untouched = preset.clone();
    if let Some(memory) = untouched.bend.memory.as_mut() {
        memory.reaches_jpeg = false;
    }
    loaded.load(&untouched);
    assert!(!loaded.reaches_jpeg);

    // Off, the bend keeps the switch's position for when it comes back on.
    let mut off = thrown.clone();
    off.enabled = false;
    assert_eq!(off.bend(), None);
    off.load(&Preset::new("a520"));
    assert!(
        off.reaches_jpeg,
        "a preset without the bend keeps the switch"
    );
    off.enabled = true;
    assert_eq!(off.bend(), thrown.bend());

    // The shipped presets that throw it.
    for name in ["a520-card-rot", "a520-strip-repeat"] {
        let shipped =
            presets::load_preset(&root().join(format!("presets/{name}.toml"))).expect("shipped");
        let panel = MemoryPanel::from_preset(&shipped);
        assert!(panel.enabled && panel.reaches_jpeg, "{name}");
        let mut again = shipped.clone();
        panel.apply(&mut again);
        assert_eq!(
            again, shipped,
            "{name}: the panel writes back what it loaded"
        );
    }

    // The strip lights the JPEG through the memory panel while the switch is thrown.
    assert!(Stage::Jpeg.is_bent(&preset));
    assert_eq!(Stage::Jpeg.panel(&preset), PanelId::Memory);
    assert!(!Stage::Jpeg.is_bent(&untouched));
    assert_eq!(Stage::Jpeg.panel(&untouched), PanelId::Jpeg);
}

#[test]
fn the_data_pin_grid_names_each_pin_once() {
    let count = data_pins();
    let mut panel = memory_panel();
    let data = &mut panel.data;
    assert_eq!(data.bridged_by(6), Some(3), "d3 bridges d6");
    assert!(
        !data.set_kind(6, Some(FaultKind::Low), count),
        "the other end of a bridge carries that bridge"
    );
    assert!(
        !data.set_partner(3, 9, count),
        "d9 carries a fault of its own"
    );
    assert!(
        !data.set_partner(3, 3, count),
        "a bridge to its own pin is refused"
    );
    assert!(
        !data.set_partner(3, count, count),
        "d16 is beyond the SDRAM"
    );
    let partners = data.partners(3, count);
    assert!(partners.contains(&6), "the present partner stays on offer");
    assert!(
        !partners.contains(&9) && !partners.contains(&12) && !partners.contains(&3),
        "faulted pins and the pin itself are not: {partners:?}"
    );
    assert!(data.set_partner(3, 4, count));
    assert!(data.is_free(6), "d6 is free again once d3 bridges d4");
    assert_eq!(data.bridged_by(4), Some(3));
    assert!(data.set_kind(3, None, count), "d3 back to sound");
    assert!(data.is_free(4), "a sound pin frees its partner");
    assert!(data.set_kind(3, Some(FaultKind::Bridge), count));
    assert_eq!(
        data.setting(3).with,
        4,
        "bridged again, it joins the pin it last joined"
    );

    let mut grids = MemoryPanel {
        enabled: true,
        address: PinGrid::default(),
        ..MemoryPanel::default()
    };
    assert!(
        grids
            .address
            .set_kind(3, Some(FaultKind::Bridge), ADDRESS_PINS)
    );
    assert_eq!(grids.address.setting(3).with, 4, "a3 bridges a4");
    assert!(
        grids.data.set_kind(3, Some(FaultKind::Bridge), count),
        "a3 and d3 are different pins"
    );
    assert_eq!(
        grids.data.setting(3).with,
        4,
        "d4 is free whatever a4 carries"
    );
    let preset = written(|preset| grids.apply(preset));
    let bend = preset.bend.memory.expect("the memory bend");
    assert_eq!(bend.faults.len(), 1);
    assert_eq!(bend.data_faults.len(), 1);
}

#[test]
fn random_edits_of_the_data_grid_never_name_a_pin_twice() {
    let count = data_pins();
    let mut grid = PinGrid::default();
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = |below: u64| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (state >> 33) % below
    };
    let kinds = [
        None,
        Some(FaultKind::Low),
        Some(FaultKind::High),
        Some(FaultKind::Bridge),
    ];
    let mut refused = 0;
    for step in 0..2000 {
        let pin = next(u64::from(count)) as u8;
        let changed = match next(3) {
            0 => grid.set_kind(pin, kinds[next(4) as usize], count),
            1 => grid.set_partner(pin, next(u64::from(count) + 1) as u8, count),
            _ => grid.set_marginal(pin, next(2) == 1),
        };
        refused += usize::from(!changed);
        let faults: Vec<DataFault> = grid
            .pins
            .iter()
            .filter_map(PinSetting::data_fault)
            .collect();
        let names = named(&faults);
        let unique: HashSet<u8> = names.iter().copied().collect();
        assert_eq!(
            unique.len(),
            names.len(),
            "step {step}: a pin is named twice: {faults:?}"
        );
        assert!(
            names.iter().all(|&pin| pin < count),
            "step {step}: every named pin exists: {faults:?}"
        );
        for fault in &faults {
            if fault.kind == FaultKind::Bridge {
                assert_eq!(
                    grid.bridged_by(fault.with.expect("a bridge names its partner")),
                    Some(fault.pin),
                    "step {step}: the partner shows the bridge"
                );
            }
        }
        if step % 100 == 0 {
            let panel = MemoryPanel {
                enabled: true,
                data: grid.clone(),
                temperature_c: Some(90.0),
                ..MemoryPanel::default()
            };
            written(|preset| panel.apply(preset));
        }
    }
    assert!(
        refused > 100,
        "the walk tried edits the grid refused: {refused}"
    );
}

#[test]
fn the_data_pins_name_the_sample_bits_they_carry() {
    let camera = camera();
    let bits = camera.adc.dsp_bits;
    assert_eq!(bits, 10, "the A520 keeps a 10-bit sample");
    assert!(
        memory::data_bit(bits, 9).starts_with("d9 carries bit 9 of the 10-bit sample, 512 codes"),
        "{}",
        memory::data_bit(bits, 9)
    );
    assert!(
        memory::data_bit(bits, 5).contains("32 codes"),
        "d5 high lifts black by 32 codes"
    );
    assert!(memory::data_bit(bits, 0).contains(", 1 code:"));
    for pin in bits..camera.memory.data_pins {
        assert!(
            memory::data_bit(bits, pin).contains("no bit"),
            "d{pin} carries no sample bit on the A520"
        );
    }
}

#[test]
fn the_retention_readout_follows_the_log_normal_law() {
    let close = |a: f64, b: f64, tolerance: f64| (a - b).abs() <= tolerance * b.abs();
    assert!(close(memory::normal_cdf(0.0), 0.5, 1e-7));
    assert!(close(memory::normal_cdf(1.0), 0.841_344_746, 1e-6));
    assert!(close(memory::normal_cdf(-1.959_964), 0.025, 1e-5));
    assert!(
        close(memory::normal_cdf(-7.0), 1.279_812_5e-12, 1e-5),
        "the tail keeps its digits: {}",
        memory::normal_cdf(-7.0)
    );
    for x in [0.3, 1.7, 3.2, 5.5] {
        assert!(
            close(memory::normal_cdf(-x) + memory::normal_cdf(x), 1.0, 1e-7),
            "symmetric at {x}"
        );
    }

    let camera = camera();
    let m = &camera.memory;
    assert!(close(
        memory::median_retention_s(m, m.reference_temp_c),
        m.retention_s,
        1e-12
    ));
    assert!(
        close(
            memory::median_retention_s(m, m.reference_temp_c + m.retention_halving_c),
            m.retention_s / 2.0,
            1e-12
        ),
        "one halving step above the reference halves the median"
    );
    let refresh_s = m.refresh_ms * 1e-3;
    let at_refresh =
        m.reference_temp_c + m.retention_halving_c * (m.retention_s / refresh_s).log2();
    assert!(
        close(memory::weak_fraction(m, at_refresh), 0.5, 1e-6),
        "half the cells leak where the median reaches the refresh period"
    );
    let one_sigma =
        at_refresh - m.retention_halving_c * m.retention_spread / std::f64::consts::LN_2;
    assert!(
        close(memory::weak_fraction(m, one_sigma), 0.158_655_25, 1e-5),
        "one spread above the refresh period, Φ(-1) of the cells leak"
    );
    let mut last = 0.0;
    for temperature_c in (-20..=120).step_by(5) {
        let fraction = memory::weak_fraction(m, f64::from(temperature_c));
        assert!(
            fraction >= last,
            "the share grows with heat: {fraction} at {temperature_c} °C"
        );
        last = fraction;
    }

    let cells = memory::cell_count(m);
    assert!(
        memory::weak_fraction(m, m.reference_temp_c) * cells < 1e-6,
        "no cell of the chip leaks at the reference"
    );
    let hot = memory::weak_sample_fraction(&camera, 120.0);
    assert!(
        (0.15..0.25).contains(&hot),
        "about a fifth of the samples hold a leaking cell at 120 °C: {hot}"
    );
    let warm = memory::weak_sample_fraction(&camera, 110.0);
    assert!(
        (1e-3..1e-2).contains(&warm),
        "a few thousandths at 110 °C: {warm}"
    );

    assert_eq!(
        memory::retention_readout(&camera, m.reference_temp_c),
        "Median retention 128 s against the 64 ms refresh: no cell of the 134 million leaks before it comes."
    );
    assert_eq!(
        memory::retention_readout(&camera, 95.0),
        "Median retention 1.00 s against the 64 ms refresh: about 3 of the 134 million cells leak before it comes."
    );
    assert_eq!(
        memory::retention_readout(&camera, 120.0),
        "Median retention 177 ms against the 64 ms refresh: 2.11 % of the cells leak before it comes, in 19.2 % of the samples."
    );

    let knob = &memory::TEMPERATURE;
    assert_eq!(knob, &sensor::TEMPERATURE, "the sensor bend's knob");
    for temperature_c in [m.reference_temp_c, 110.0, 120.0] {
        assert!(knob.range.contains(&temperature_c));
    }
}

#[test]
fn the_serial_readout_counts_the_bits_and_the_corrupted_writes() {
    let camera = camera();
    let close = |a: f64, b: f64| (a - b).abs() <= 1e-9 * b.abs().max(1e-12);
    let data_bits = camera.afe.serial_data_bits;
    assert_eq!(
        afe::serial_bits_per_frame(&camera),
        7 * (8 + u32::from(data_bits))
    );
    assert_eq!(
        afe::serial_bits_per_frame(&camera),
        224,
        "the A520's 24-bit field"
    );

    let default = SerialBend::default();
    let expected = 7.0 * (1.0 - (1.0 - default.bit_error).powi(8 + i32::from(data_bits)));
    assert!(close(afe::corrupted_writes(&camera, &default), expected));
    assert!(close(
        afe::corrupted_writes(&camera, &SerialBend { bit_error: 1.0 }),
        7.0
    ));
    let rare = SerialBend { bit_error: 1e-7 };
    let linear = 7.0 * f64::from(8 + data_bits) * rare.bit_error;
    assert!(
        (afe::corrupted_writes(&camera, &rare) - linear).abs() < 1e-3 * linear,
        "rare errors corrupt about one write per error"
    );
    assert_eq!(
        afe::serial_readout(&camera, &default),
        "224 bits a frame in 7 writes of 8 + 24; on average 0.221 writes corrupted a frame, at least one in 20.1 % of the frames."
    );
    assert_eq!(
        afe::serial_readout(&camera, &SerialBend { bit_error: 0.0 }),
        "224 bits a frame in 7 writes of 8 + 24; none is corrupted."
    );

    let knob = &afe::BIT_ERROR;
    assert_eq!(knob.format(knob.scale.show(0.001)), "1.00 per 1000 bits");
    assert!(close(knob.scale.store(*knob.range.start()), 1e-5));
    assert!(close(knob.scale.store(*knob.range.end()), 1.0));
    assert!(knob.log, "the bit error turns over five decades");
    assert!(knob.range.contains(&knob.scale.show(default.bit_error)));
}

#[test]
fn the_leak_readout_and_knobs_follow_the_edge() {
    let camera = camera();
    let default = LeakBend::default();
    assert_eq!(
        sensor::leak_readout(&camera, &default),
        "0.500 × white at the left edge, 1/e after 682 of the sensor's 2272 columns, 3.57 % at the right edge."
    );
    let top = LeakBend {
        edge: Edge::Top,
        spread_frac: 0.5,
        ..default
    };
    assert_eq!(
        sensor::leak_readout(&camera, &top),
        "0.500 × white at the top edge, 1/e after 852 of the sensor's 1704 rows, 13.5 % at the bottom edge."
    );
    let veil = LeakBend {
        edge: Edge::Right,
        spread_frac: 2.0,
        radiance: 3.0,
        ..default
    };
    assert!(
        sensor::leak_readout(&camera, &veil).contains("1/e beyond the sensor's 2272 columns"),
        "{}",
        sensor::leak_readout(&camera, &veil)
    );
    assert!(
        (sensor::far_edge_share(&veil) - (-0.5f64).exp()).abs() < 1e-12,
        "a veil keeps most of its light across the frame"
    );
    assert_eq!(
        sensor::leak_readout(
            &camera,
            &LeakBend {
                radiance: 0.0,
                ..default
            }
        ),
        "No light gets in."
    );

    let edges: Vec<Edge> = sensor::EDGES.iter().map(|&(edge, _)| edge).collect();
    assert_eq!(edges, [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom]);
    for (edge, label) in sensor::EDGES {
        assert_eq!(
            label,
            edge.to_string(),
            "the choice reads as the preset's key"
        );
        assert_eq!(sensor::opposite(sensor::opposite(edge)), edge);
    }
    assert!(sensor::spread_knob(Edge::Left).unit.contains("width"));
    assert!(sensor::spread_knob(Edge::Bottom).unit.contains("height"));
    assert_eq!(sensor::RADIANCE.format(0.5), "0.500 × white");
    assert_eq!(
        sensor::SPREAD_WIDTH.format(sensor::SPREAD_WIDTH.scale.show(0.3)),
        "30.0 % of width"
    );
    assert_eq!(sensor::COLOUR.format(1.0), "1.00 × white");
    assert_eq!(sensor::NIR.format(1.0), "1.00 × daylight");
}

#[test]
fn the_knobs_span_the_defaults_and_the_core_accepts_their_ends() {
    let camera = camera();
    let leak = LeakBend::default();
    let serial = SerialBend::default();
    for (name, knob, value) in [
        ("radiance", &sensor::RADIANCE, leak.radiance),
        ("spread", &sensor::SPREAD_WIDTH, leak.spread_frac),
        ("spread", &sensor::SPREAD_HEIGHT, leak.spread_frac),
        ("red", &sensor::COLOUR, leak.colour[0]),
        ("near infrared", &sensor::NIR, leak.nir),
        ("bit error", &afe::BIT_ERROR, serial.bit_error),
        (
            "SDRAM temperature",
            &memory::TEMPERATURE,
            camera.memory.reference_temp_c,
        ),
    ] {
        let shown = knob.scale.show(value);
        assert!(
            knob.range.contains(&shown),
            "{name}: the default {shown} lies outside {:?}",
            knob.range
        );
        assert!(!knob.unit.trim().is_empty(), "{name} has a unit");
    }

    let top = data_pins() - 1;
    let ends = [
        (
            Edge::Left,
            *sensor::RADIANCE.range.start(),
            *sensor::SPREAD_WIDTH.range.start(),
            *sensor::COLOUR.range.start(),
            *sensor::NIR.range.start(),
            *afe::BIT_ERROR.range.start(),
            *memory::TEMPERATURE.range.start(),
        ),
        (
            Edge::Bottom,
            *sensor::RADIANCE.range.end(),
            *sensor::SPREAD_HEIGHT.range.end(),
            *sensor::COLOUR.range.end(),
            *sensor::NIR.range.end(),
            *afe::BIT_ERROR.range.end(),
            *memory::TEMPERATURE.range.end(),
        ),
    ];
    for (edge, radiance, spread, colour, nir, bit_error, temperature_c) in ends {
        let mut memory = MemoryPanel {
            enabled: true,
            temperature_c: Some(temperature_c),
            ..MemoryPanel::default()
        };
        assert!(memory.data.set_kind(0, Some(FaultKind::High), top + 1));
        assert!(memory.data.set_kind(top, Some(FaultKind::Low), top + 1));
        let panels = Panels {
            leak: LeakPanel {
                enabled: true,
                bend: LeakBend {
                    edge,
                    radiance: sensor::RADIANCE.scale.store(radiance),
                    spread_frac: sensor::SPREAD_WIDTH.scale.store(spread),
                    colour: [sensor::COLOUR.scale.store(colour); 3],
                    nir: sensor::NIR.scale.store(nir),
                },
            },
            serial: SerialPanel {
                enabled: true,
                bend: SerialBend {
                    bit_error: afe::BIT_ERROR.scale.store(bit_error),
                },
            },
            memory,
            ..Panels::default()
        };
        let mut preset = Preset::new("a520");
        preset.readout.resolution = Resolution::MatchInput;
        panels.apply(&mut preset);
        let preset = through_toml(&preset);
        let scene = Scene::from_srgb8(48, 32, &[140; 48 * 32 * 3]);
        let mut simulator = Simulator::builder((camera.clone(), preset))
            .input_size(48, 32)
            .build()
            .expect("the core accepts the knobs' ends");
        let frame = simulator.frame(&scene, 0);
        assert_eq!((frame.width(), frame.height()), (48, 32));
    }
}

#[test]
fn the_strip_lights_the_stages_of_the_new_sections() {
    let with = |bend: fn(&mut Preset)| {
        let mut preset = Preset::new("a520");
        bend(&mut preset);
        preset
    };
    let lit = |preset: &Preset| {
        Stage::ALL
            .into_iter()
            .filter(|stage| stage.is_bent(preset))
            .collect::<Vec<_>>()
    };
    let leak = with(|p| p.bend.leak = Some(LeakBend::default()));
    assert_eq!(
        lit(&leak),
        [Stage::Sensor],
        "a light leak lights the sensor"
    );
    assert_eq!(Stage::Sensor.panel(&leak), PanelId::Sensor);

    let serial = with(|p| p.bend.serial = Some(SerialBend::default()));
    assert_eq!(lit(&serial), [Stage::Afe], "the serial link lights the AFE");
    assert_eq!(Stage::Afe.panel(&serial), PanelId::Afe);
    let railed = with(|p| {
        p.bend.serial = Some(SerialBend::default());
        p.bend.rail = Some(RailBend::default());
    });
    assert_eq!(
        Stage::Afe.panel(&railed),
        PanelId::Afe,
        "the serial link, the AFE's own, comes before the rail"
    );

    let heated = with(|p| {
        p.bend.memory = Some(MemoryBend {
            temperature_c: Some(110.0),
            ..MemoryBend::default()
        });
    });
    assert_eq!(
        lit(&heated),
        [Stage::Memory],
        "a heated SDRAM lights the memory"
    );
    let stuck = with(|p| {
        p.bend.memory = Some(MemoryBend {
            data_faults: vec![DataFault {
                pin: 9,
                kind: FaultKind::High,
                with: None,
                on_frac: None,
                switch_hz: None,
            }],
            ..MemoryBend::default()
        });
    });
    assert_eq!(
        lit(&stuck),
        [Stage::Memory],
        "a stuck data pin lights the memory"
    );
    assert_eq!(Stage::Memory.panel(&stuck), PanelId::Memory);
}

/// A small running job at match-input resolution with the three new sections on.
fn bent_job() -> Job {
    let mut preset = Preset::new("a520");
    preset.seed = 3;
    preset.readout.resolution = Resolution::MatchInput;
    leak_panel().apply(&mut preset);
    serial_panel().apply(&mut preset);
    memory_panel().apply(&mut preset);
    let (width, height) = (128, 96);
    let rgb: Vec<u8> = (0..width * height)
        .flat_map(|i| {
            let x = (i % width * 255 / width) as u8;
            [x, 200, 255 - x]
        })
        .collect();
    Job {
        generation: 1,
        scene: Arc::new(Scene::from_srgb8(width, height, &rgb)),
        camera: Arc::new(camera()),
        preset,
        preview: false,
        power_cycle: 0,
    }
}

/// A knob's name and the change of the preset it makes.
type Turn = (&'static str, fn(&mut Preset));

#[test]
fn turning_the_new_knobs_keeps_the_camera_running() {
    let mut renderer = Renderer::default();
    let mut job = bent_job();
    let first = renderer
        .render(&job)
        .outcome
        .expect("the bent camera renders");
    assert_eq!(first.reconfigured, Reconfigured::Rebuilt, "a new simulator");
    let kept = Reconfigured::Kept {
        bus_reset: false,
        servo_reset: false,
    };
    let turns: [Turn; 12] = [
        ("the leak's edge", |p| {
            if let Some(leak) = p.bend.leak.as_mut() {
                leak.edge = Edge::Right;
            }
        }),
        ("the leak's radiance", |p| {
            if let Some(leak) = p.bend.leak.as_mut() {
                leak.radiance = 4.0;
            }
        }),
        ("the leak's colour", |p| {
            if let Some(leak) = p.bend.leak.as_mut() {
                leak.colour = [0.2, 0.9, 1.6];
            }
        }),
        ("the bit error", |p| {
            if let Some(serial) = p.bend.serial.as_mut() {
                serial.bit_error = 0.05;
            }
        }),
        ("the SDRAM's temperature", |p| {
            if let Some(memory) = p.bend.memory.as_mut() {
                memory.temperature_c = Some(120.0);
            }
        }),
        ("a data pin", |p| {
            if let Some(memory) = p.bend.memory.as_mut() {
                memory.data_faults.remove(0);
            }
        }),
        ("the temperature's reset", |p| {
            if let Some(memory) = p.bend.memory.as_mut() {
                memory.temperature_c = None;
            }
        }),
        ("the compressed buffer's switch", |p| {
            if let Some(memory) = p.bend.memory.as_mut() {
                memory.reaches_jpeg = true;
            }
        }),
        ("the compressed buffer's switch, back", |p| {
            if let Some(memory) = p.bend.memory.as_mut() {
                memory.reaches_jpeg = false;
            }
        }),
        ("the leak's switch", |p| p.bend.leak = None),
        ("the serial link's switch", |p| p.bend.serial = None),
        ("the serial link's switch, on again", |p| {
            p.bend.serial = serial_panel().bend();
        }),
    ];
    for (index, (name, turn)) in turns.into_iter().enumerate() {
        turn(&mut job.preset);
        job.generation += 1;
        let output = renderer.render(&job).outcome.expect("renders");
        assert_eq!(
            output.reconfigured, kept,
            "turning {name} keeps every buffer and state, so the status line reports no circuit reset"
        );
        assert_eq!(output.index, index as u64 + 1, "{name}: the next frame");
    }
}
