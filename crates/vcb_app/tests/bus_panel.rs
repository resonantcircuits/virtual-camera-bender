//! The bus front panel builds presets that survive TOML and reproduce the shipped ones.

use std::fs;
use std::path::Path;

use vcb_app::panels::bus::{BusPanel, Gate, Route, significant};
use vcb_app::presets;
use vcb_core::Preset;
use vcb_core::bus::{Function, PotNode, Target};

fn root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

/// Builds a preset from `panel`, writes it as TOML, reads it back and checks both ends.
fn round_trip(panel: &BusPanel) -> Preset {
    let mut preset = Preset::new("a520");
    preset.seed = 7;
    panel.apply(&mut preset);
    let text = presets::to_toml(&preset).expect("the preset serialises");
    let parsed = presets::from_toml(&text).expect("the written preset parses and validates");
    assert_eq!(parsed, preset, "the preset survives TOML:\n{text}");
    assert_eq!(
        &BusPanel::from_preset(&parsed),
        panel,
        "the panel reads back its own preset"
    );
    parsed
}

fn enabled() -> BusPanel {
    BusPanel {
        enabled: true,
        ..BusPanel::default()
    }
}

#[test]
fn every_function_round_trips_through_toml() {
    for (route, gate, function) in [
        (Route::Bypass, Gate::Invert, Function::Bypass),
        (Route::Function, Gate::Invert, Function::Invert),
        (Route::Function, Gate::Divide, Function::Divide),
    ] {
        let panel = BusPanel {
            route,
            gate,
            ..enabled()
        };
        let preset = round_trip(&panel);
        let bus = preset
            .bend
            .bus
            .expect("an enabled panel patches the bend in");
        assert_eq!(bus.function, function);
    }
}

#[test]
fn component_values_and_switches_round_trip_through_toml() {
    let mut panel = BusPanel {
        target: Target::Gnd,
        cap_uf: 0.047,
        pot_kohm: 0.733,
        pot_node: PotNode::F,
        series_ohm: 470.0,
        common_bus: true,
        ..enabled()
    };
    panel.set_source(2, true);
    panel.set_source(11, true);
    panel.set_source(10, false);
    let preset = round_trip(&panel);
    let bus = preset.bend.bus.expect("enabled");
    assert_eq!(
        bus.source_pins,
        vec![11, 9, 2],
        "pins are listed highest first"
    );
    assert_eq!(bus.target, Target::Gnd);
    assert_eq!(bus.pot_node, PotNode::F);
    assert!(bus.common_bus);
}

#[test]
fn switching_the_bend_off_removes_it_but_keeps_the_settings() {
    let panel = BusPanel {
        cap_uf: 2.2,
        ..BusPanel::default()
    };
    let mut preset = Preset::new("a520");
    panel.apply(&mut preset);
    assert!(
        preset.bend.bus.is_none(),
        "a switched-off bend is not in the preset"
    );
    let text = presets::to_toml(&preset).expect("serialises");
    assert_eq!(presets::from_toml(&text).expect("parses"), preset);

    let mut reloaded = panel.clone();
    reloaded.enabled = true;
    reloaded.load(&preset);
    assert!(
        !reloaded.enabled,
        "loading a preset without a bend switches it off"
    );
    assert!(
        (reloaded.cap_uf - 2.2).abs() < f64::EPSILON,
        "the panel keeps its values for when the bend is switched on again"
    );
}

#[test]
fn the_gate_switch_keeps_its_position_while_bypassed() {
    let bypassed = round_trip(&BusPanel {
        route: Route::Bypass,
        ..enabled()
    });
    let mut panel = BusPanel {
        route: Route::Function,
        gate: Gate::Divide,
        ..enabled()
    };
    panel.load(&bypassed);
    assert_eq!(panel.function(), Function::Bypass);
    panel.route = Route::Function;
    assert_eq!(
        panel.function(),
        Function::Divide,
        "a bypassed preset does not move the Invert / Divide switch"
    );
}

#[test]
fn the_panel_reproduces_every_shipped_preset() {
    let dir = root().join("presets");
    let entries = presets::discover(&dir).expect("the presets folder is readable");
    assert!(
        entries.len() >= 4,
        "the shipped presets are found: {entries:?}"
    );
    for entry in entries {
        let text = fs::read_to_string(&entry.path).expect("readable");
        let preset = presets::from_toml(&text).expect("the shipped preset is valid");
        let mut rebuilt = preset.clone();
        BusPanel::from_preset(&preset).apply(&mut rebuilt);
        assert_eq!(rebuilt, preset, "{} survives the front panel", entry.name);
    }
}

#[test]
fn source_pins_outside_the_bank_are_not_shown() {
    let mut panel = enabled();
    panel.sources = 0;
    panel.set_source(5, true);
    assert!(panel.source(5));
    assert!(!panel.source(1), "D1 is not on the DIP bank");
    assert!(!panel.source(12), "D12 is not on the DIP bank");
}

#[test]
fn values_show_three_significant_digits() {
    assert_eq!(significant(1.0, 3), "1.00");
    assert_eq!(significant(0.047, 3), "0.0470");
    assert_eq!(significant(220.0, 3), "220");
    assert_eq!(significant(4700.0, 3), "4700");
}
