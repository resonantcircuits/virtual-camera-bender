# Rust engineering standards for this repository

This project follows Microsoft's Pragmatic Rust Guidelines
(<https://microsoft.github.io/rust-guidelines/guidelines/index.html>) on top of the
upstream Rust API Guidelines, Style Guide and Design Patterns. The full guideline
text is mirrored locally at `docs/research/downloads/rust-guidelines-all.txt`
(gitignored; re-download from <https://microsoft.github.io/rust-guidelines/agents/all.txt>).

This file is the project digest: the rules that apply here, with the concrete
settings we use. Identifiers such as `M-ERRORS-CANONICAL-STRUCTS` refer to the
guideline items. When in doubt, read the item; it is the spirit that counts.

## Workspace and crates (M-CARGO-WORKSPACE, M-CRATES-IN-WORKSPACE, M-CRATES-FLAT-FOLDER, M-SMALLER-CRATES)

- One workspace `Cargo.toml` at the repo root; every crate lives directly under `crates/`.
- Shared metadata, dependency versions and lints come from `[workspace.*]` tables.
  Workspace dependency entries do not enable features (except `std`); crates opt in.
- Intra-workspace dependencies use `name.workspace = true`.
- Crate names are prefixed `vcb_` and express relationships by name (`vcb_core`, `vcb_io`, `vcb_cli`).
- Edition 2024 (M-LATEST-EDITION). MSRV is declared with `rust-version` and kept a few
  versions behind the current stable (M-MSRV).
- Split a crate when a submodule can stand alone; features are additive only (M-FEATURES-ADDITIVE).
- Test-only functionality (mocks, fake data, inspection of internals) sits behind a
  single feature named `test-util` (M-TEST-UTIL).

## Lints (M-STATIC-VERIFICATION, M-LINT-OVERRIDE-EXPECT)

The workspace enables, at `warn`, these compiler lints: `ambiguous_negative_literals`,
`missing_debug_implementations`, `redundant_imports`, `redundant_lifetimes`,
`trivial_numeric_casts`, `unused_lifetimes`, `unsafe_op_in_unsafe_fn`.

Clippy groups at `warn` (priority -1): `cargo`, `complexity`, `correctness`, `pedantic`,
`perf`, `style`, `suspicious`. Restriction lints at `warn`:
`allow_attributes_without_reason`, `as_pointer_underscore`, `assertions_on_result_states`,
`clone_on_ref_ptr`, `deref_by_slicing`, `disallowed_script_idents`, `empty_drop`,
`empty_enum_variants_with_brackets`, `empty_structs_with_brackets`, `fn_to_numeric_cast_any`,
`if_then_some_else_none`, `map_err_ignore`, `redundant_type_annotations`,
`renamed_function_params`, `semicolon_outside_block`, `unneeded_field_pattern`,
`unused_result_ok`, `too_long_first_doc_paragraph`, `undocumented_unsafe_blocks`,
`unnecessary_safety_comment`, `unnecessary_safety_doc`.

Project-wide exceptions (the guidelines explicitly allow opting out of numeric-cast
lints for signal-processing code): `cast_possible_truncation`, `cast_precision_loss`,
`cast_sign_loss`, `cast_lossless` and `similar_names` are `allow` at workspace level,
with the reason recorded next to the setting. `module_name_repetitions` is allowed.

The CI gate is `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`,
`cargo test --all-features`, and `cargo doc --no-deps` with no warnings.
Local overrides use `#[expect(lint, reason = "...")]`, never bare `#[allow]`.

## Errors and panics

- Library crates (`vcb_core`, `vcb_io`) expose canonical error structs
  (M-ERRORS-CANONICAL-STRUCTS): a `pub struct Error` holding a private `ErrorKind`,
  a `Backtrace`, an optional source, and `is_*()` accessors. `Display` prints a summary
  sentence. Implement `std::error::Error`, `Debug`, `Display`.
- Conversions use `impl From<Upstream> for Error` and `?` (M-FROM-ERROR); `map_err`
  only to add context or for foreign types.
- Applications (`vcb_cli`) use `anyhow` (M-APP-ERROR) and set `mimalloc` as the global
  allocator (M-MIMALLOC-APPS).
- Programming bugs panic with a message that states the actual values
  (M-PANIC-ON-BUG, M-PANIC-MESSAGE). Fallible input (files, presets, user parameters)
  returns `Result`. Panics are never used to report errors (M-PANIC-IS-STOP).
- No `unsafe` without a benchmark-backed or FFI reason, a `// SAFETY:` comment and a
  Miri run (M-UNSAFE, M-UNSOUND). The first milestone contains no `unsafe`.

## API design

- Public types derive `Debug` (M-PUBLIC-DEBUG) and the common traits where sensible
  (`Clone`, `Copy`, `PartialEq`, `Default`, `Hash`) (C-COMMON-TRAITS). Types meant to be
  read implement `Display` (M-PUBLIC-DISPLAY).
- Short names, at most two words, no module prefix in the name, no weasel words such as
  `Manager`, `Service`, `Factory` (M-SHORT-NAMES, M-WEASEL-WORDS). Getters have no
  `get_` prefix (C-GETTER).
- Construction: up to two optional parameters via `new`/`with_*`; beyond that a builder
  named `FooBuilder` reached only through `Foo::builder(deps)`, with setters named after
  the field and validation in `build() -> Result` (M-INIT-BUILDER, M-BUILD-RESULT).
  Group related parameters into structs instead of long signatures (M-INIT-CASCADED).
- Prefer concrete types, then generics, then `dyn Trait` (M-DI-HIERARCHY). Avoid
  `Arc`, `Box`, `RefCell` in public signatures (M-AVOID-WRAPPERS). No nested generic
  service types (M-SIMPLE-ABSTRACTIONS).
- Essential functionality is inherent; trait impls forward to it (M-ESSENTIAL-FN-INHERENT).
- Accept `impl AsRef<Path>`, `impl AsRef<str>`, `impl RangeBounds<T>` where feasible
  (M-IMPL-ASREF, M-IMPL-RANGEBOUNDS). Libraries are sans-I/O: `vcb_core` never touches
  files, clocks or entropy; it takes buffers and seeds (M-IMPL-IO, M-MOCKABLE-SYSCALLS).
- Do not leak third-party types from public APIs unless behind a feature (`serde` is
  behind the `serde` feature in `vcb_core`) (M-DONT-LEAK-TYPES, M-FOREIGN-REEXPORTS).
- Each public item is reachable through exactly one path; no preludes, no glob
  re-exports (M-SINGLE-ITEM-PATH, M-NO-PRELUDE, M-NO-GLOB-REEXPORTS).
- Modules are balanced: essential items at the crate root, the rest grouped by use case,
  never by kind (`traits.rs`, `errors.rs` are not module names) (M-BALANCED-MODULES).
- Newtypes that encode an invariant enforce it in a fallible constructor
  (M-STRONG-TYPES-GUARD). Plain numeric parameters stay plain numbers (M-STRONG-TYPES).
- Statics are avoided; state lives in the `Simulator` (M-AVOID-STATICS).
- Public types are `Send` (M-TYPES-SEND).

## Physical units convention (project specific)

Configuration and parameter structs use plain `f64`/`f32` fields whose names carry the
unit as a suffix: `pot_kohm`, `cap_uf`, `series_ohm`, `freq_hz`, `amplitude_v`,
`pixel_clock_hz`, `exposure_s`, `full_well_e`, `pitch_um`. Internal math converts to SI
at the boundary of the stage that consumes the value. Dimensionless ratios are named
for what they are (`cte`, `antibloom_ratio`), never `amount` or `strength`.

Magic numbers are named constants with a doc comment stating why the value was chosen
and what depends on it (M-DOCUMENTED-MAGIC). A physics constant also cites its source.

## Documentation (M-MODULE-DOCS, M-CANONICAL-DOCS, M-FIRST-DOC-SENTENCE, M-DOC-INLINE)

- Every public module has `//!` docs: what it contains, when to use it, the physical
  model it implements, observable side effects.
- Every public item has a summary sentence under 15 words, then free-form detail, then
  `# Examples`, `# Errors`, `# Panics` as applicable. No parameter tables.
- Docs describe the end state, not the design history (M-NO-META-DESIGN-DOCUMENTATION).
  Design rationale lives in `docs/ARCHITECTURE.md`, not in rustdoc.
- `pub use` of crate items is marked `#[doc(inline)]`.

## Testing (M-INTEGRATION-TESTS, M-TAUTOLOGICAL-TESTS)

- Tests of the public API live under `tests/` in each crate; `mod tests` inside `src/`
  is for genuinely private helpers only.
- Tests assert properties and behaviour (determinism, invariants, physical expectations
  such as "a 1 kHz injection at a 130 µs line period produces about 7.7 lines per
  cycle"), never restate constants.
- Golden-image tests (`crates/vcb_core/tests/golden.rs`) render a synthetic 160 by 120
  scene through each shipped preset (still mode, match-input resolution, seed 1) and
  compare a 64-bit FNV-1a hash of the frame's RGB bytes with the committed
  `tests/golden/<preset>.hash`; a PNG of each golden frame sits beside it for review.
  After an intended change of the output, run
  `VCB_UPDATE_GOLDEN=1 cargo test -p vcb_core --test golden -- --nocapture`, which
  rewrites the hashes and PNGs instead of asserting, and review the PNGs before
  committing them.
- Benchmarks use `divan` for the hot stages (M-HOTPATH).

## Performance (M-THROUGHPUT, M-MEM-REUSE, M-INITIAL-CAPACITY, M-BOX-DST)

- Image planes are `Box<[f32]>` (immutable after construction) or reused `Vec<f32>`
  scratch buffers owned by the `Simulator`; no per-frame reallocation in the steady state.
- Collections are created with capacity. Hot loops contain no logging and no allocation.
- Row-parallel work uses `rayon`; the row-sequential circuit stages run per row with
  carried state where the physics allows it, and sequentially otherwise.
- Hashing of trusted keys uses a fast hasher (`foldhash`) when hashing is needed at all.

## Logging (M-LOG-STRUCTURED, M-LOG-NOT-PRINT, M-LOG-OVERHEAD)

Libraries emit `tracing` events with dot-separated names (`sim.frame.done`) and named
properties; never `println!`. The CLI writes its user interface to stdout/stderr
directly. No telemetry inside per-pixel or per-sample loops.
