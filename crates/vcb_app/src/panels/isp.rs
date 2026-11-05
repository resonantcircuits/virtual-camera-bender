//! The processor panel: overrides of the image processor's settings, and its JPEG encoder.
//!
//! Every setting starts at the camera's own value, shown dimmed. Moving a control
//! writes that one value into the preset's `[isp]` section; its reset button removes
//! it again, and removing the last one removes the section. Values the panel does not
//! show, such as a colour-matrix override written by hand, are kept as they are.
//!
//! Units: white-balance gains and tone contrast are factors (×), the toe and the
//! auto-exposure target are percent of reference white, the shoulder percent of the
//! output range, and blur radii (noise reduction and sharpening) pixels of the full
//! render; the preset stores the radii as fractions of the frame width, so a radius
//! keeps its look at every resolution. Servo time constants are seconds of camera time.

use eframe::egui::Ui;
use vcb_core::codec::{ChromaUp, Quality, Subsampling};
use vcb_core::isp::{AeMode, WbMode};
use vcb_core::preset::{IspOverride, Mode};
use vcb_core::{Camera, Preset};

use crate::panels::{self, Edit, Knob, PanelId, Scale};

/// A numeric processor setting the panel can override.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Field {
    /// Daylight red gain.
    WbRedGain,
    /// Daylight blue gain.
    WbBlueGain,
    /// White-balance servo time constant.
    WbTau,
    /// Tone curve slope at mid-grey.
    ToneContrast,
    /// Tone curve toe.
    ToneToe,
    /// Tone curve shoulder.
    ToneShoulder,
    /// Chroma noise reduction radius.
    ChromaNr,
    /// Luma noise reduction radius.
    LumaNr,
    /// Unsharp-mask radius.
    SharpenRadius,
    /// Unsharp-mask gain.
    SharpenGain,
    /// Auto-exposure target.
    AeTarget,
    /// Auto-exposure servo time constant.
    AeTau,
}

impl Field {
    /// Every field, in panel order.
    pub const ALL: [Self; 12] = [
        Self::WbRedGain,
        Self::WbBlueGain,
        Self::WbTau,
        Self::ToneContrast,
        Self::ToneToe,
        Self::ToneShoulder,
        Self::ChromaNr,
        Self::LumaNr,
        Self::SharpenRadius,
        Self::SharpenGain,
        Self::AeTarget,
        Self::AeTau,
    ];

    /// The camera's own value of the field.
    #[must_use]
    pub fn camera(self, camera: &Camera) -> f64 {
        let isp = &camera.isp;
        match self {
            Self::WbRedGain => isp.wb_r_gain,
            Self::WbBlueGain => isp.wb_b_gain,
            Self::WbTau => isp.wb_tau_s,
            Self::ToneContrast => isp.tone_contrast,
            Self::ToneToe => isp.tone_toe,
            Self::ToneShoulder => isp.tone_shoulder,
            Self::ChromaNr => isp.chroma_nr_frac,
            Self::LumaNr => isp.luma_nr_frac,
            Self::SharpenRadius => isp.sharpen_radius_frac,
            Self::SharpenGain => isp.sharpen_gain,
            Self::AeTarget => isp.ae_target,
            Self::AeTau => isp.ae_tau_s,
        }
    }

    /// The preset's override of the field.
    #[must_use]
    pub fn get(self, isp: &IspOverride) -> Option<f64> {
        match self {
            Self::WbRedGain => isp.wb_r_gain,
            Self::WbBlueGain => isp.wb_b_gain,
            Self::WbTau => isp.wb_tau_s,
            Self::ToneContrast => isp.tone_contrast,
            Self::ToneToe => isp.tone_toe,
            Self::ToneShoulder => isp.tone_shoulder,
            Self::ChromaNr => isp.chroma_nr_frac,
            Self::LumaNr => isp.luma_nr_frac,
            Self::SharpenRadius => isp.sharpen_radius_frac,
            Self::SharpenGain => isp.sharpen_gain,
            Self::AeTarget => isp.ae_target,
            Self::AeTau => isp.ae_tau_s,
        }
    }

    /// The override's slot for the field.
    fn slot(self, isp: &mut IspOverride) -> &mut Option<f64> {
        match self {
            Self::WbRedGain => &mut isp.wb_r_gain,
            Self::WbBlueGain => &mut isp.wb_b_gain,
            Self::WbTau => &mut isp.wb_tau_s,
            Self::ToneContrast => &mut isp.tone_contrast,
            Self::ToneToe => &mut isp.tone_toe,
            Self::ToneShoulder => &mut isp.tone_shoulder,
            Self::ChromaNr => &mut isp.chroma_nr_frac,
            Self::LumaNr => &mut isp.luma_nr_frac,
            Self::SharpenRadius => &mut isp.sharpen_radius_frac,
            Self::SharpenGain => &mut isp.sharpen_gain,
            Self::AeTarget => &mut isp.ae_target,
            Self::AeTau => &mut isp.ae_tau_s,
        }
    }

    /// The knob of the field; `width` is the full render's width in pixels, for radii.
    #[must_use]
    pub fn knob(self, width: f64) -> Knob {
        let pixels = Scale::Times(width.max(1.0));
        match self {
            Self::WbRedGain | Self::WbBlueGain => Knob::log(0.25..=8.0, 0.25, " ×").decimals(2),
            Self::WbTau | Self::AeTau => Knob::log(0.01..=10.0, 0.01, " s"),
            Self::ToneContrast => Knob::log(0.25..=4.0, 0.25, " ×").decimals(2),
            Self::ToneToe => Knob::linear(0.0..=20.0, " % of white", 1).scaled(Scale::Times(100.0)),
            Self::ToneShoulder => {
                Knob::linear(0.0..=100.0, " % of range", 0).scaled(Scale::Times(100.0))
            }
            Self::ChromaNr => Knob::linear(0.0..=20.0, " px", 1).scaled(pixels),
            Self::LumaNr | Self::SharpenRadius => Knob::linear(0.0..=10.0, " px", 2).scaled(pixels),
            Self::SharpenGain => Knob::linear(0.0..=4.0, " ×", 2),
            Self::AeTarget => {
                Knob::linear(1.0..=100.0, " % of white", 0).scaled(Scale::Times(100.0))
            }
        }
    }
}

/// The value of `field` in effect and whether the preset overrides it.
#[must_use]
pub fn value(isp: Option<&IspOverride>, camera: &Camera, field: Field) -> (f64, bool) {
    match isp.and_then(|isp| field.get(isp)) {
        Some(value) => (value, true),
        None => (field.camera(camera), false),
    }
}

/// Overrides `field` with `value`, creating the `[isp]` section if needed.
pub fn set(isp: &mut Option<IspOverride>, field: Field, value: f64) {
    *field.slot(isp.get_or_insert_with(IspOverride::default)) = Some(value);
}

/// Returns `field` to the camera's value; an `[isp]` section left empty is removed.
pub fn reset(isp: &mut Option<IspOverride>, field: Field) {
    if let Some(section) = isp.as_mut() {
        *field.slot(section) = None;
    }
    tidy(isp);
}

/// Sets or, with `None`, resets the white-balance mode.
pub fn set_wb(isp: &mut Option<IspOverride>, mode: Option<WbMode>) {
    if mode.is_some() {
        isp.get_or_insert_with(IspOverride::default).wb = mode;
    } else if let Some(section) = isp.as_mut() {
        section.wb = None;
        tidy(isp);
    }
}

/// Sets or, with `None`, resets the auto-exposure mode.
pub fn set_ae(isp: &mut Option<IspOverride>, mode: Option<AeMode>) {
    if mode.is_some() {
        isp.get_or_insert_with(IspOverride::default).ae_mode = mode;
    } else if let Some(section) = isp.as_mut() {
        section.ae_mode = None;
        tidy(isp);
    }
}

/// Removes an `[isp]` section that overrides nothing.
fn tidy(isp: &mut Option<IspOverride>) {
    if isp.as_ref() == Some(&IspOverride::default()) {
        *isp = None;
    }
}

/// The white-balance choices.
const WB_MODES: [(WbMode, &str); 3] = [
    (WbMode::Daylight, "Daylight"),
    (WbMode::Auto, "Auto"),
    (WbMode::Servo, "Servo"),
];

/// The auto-exposure choices.
const AE_MODES: [(AeMode, &str); 3] = [
    (AeMode::Fixed, "Fixed"),
    (AeMode::Scene, "Scene"),
    (AeMode::Servo, "Servo"),
];

/// Draws the processor panel; `width` is the full render's width, `scroll` a panel to reveal.
pub fn show(
    ui: &mut Ui,
    preset: &mut Preset,
    camera: &Camera,
    width: f64,
    scroll: Option<PanelId>,
) -> Edit {
    panels::header(ui, "Processor", None);
    let mode = preset.readout.mode;
    let mut rows = Rows {
        isp: &mut preset.isp,
        camera,
        width,
        edit: Edit::default(),
    };

    panels::section(ui, "White balance");
    panels::grid(ui, "isp-wb", |ui| {
        let mut wb = rows.isp.as_ref().and_then(|isp| isp.wb);
        let before = wb;
        panels::override_choice(
            ui,
            &mut rows.edit,
            "Mode",
            &mut wb,
            WbMode::Daylight,
            &WB_MODES,
        );
        if wb != before {
            set_wb(rows.isp, wb);
        }
        rows.field(ui, Field::WbRedGain, "Red gain");
        rows.field(ui, Field::WbBlueGain, "Blue gain");
        if wb == Some(WbMode::Servo) {
            rows.field(ui, Field::WbTau, "Servo lag");
        }
    });

    panels::section(ui, "Tone");
    panels::grid(ui, "isp-tone", |ui| {
        rows.field(ui, Field::ToneContrast, "Contrast");
        rows.field(ui, Field::ToneToe, "Toe");
        rows.field(ui, Field::ToneShoulder, "Shoulder");
    });

    panels::section(ui, "Noise reduction and sharpening");
    panels::grid(ui, "isp-detail", |ui| {
        rows.field(ui, Field::ChromaNr, "Chroma NR");
        rows.field(ui, Field::LumaNr, "Luma NR");
        rows.field(ui, Field::SharpenRadius, "Sharpen radius");
        rows.field(ui, Field::SharpenGain, "Sharpen gain");
    });

    panels::section(ui, "Auto-exposure");
    panels::grid(ui, "isp-ae", |ui| {
        let mut ae = rows.isp.as_ref().and_then(|isp| isp.ae_mode);
        let before = ae;
        panels::override_choice(
            ui,
            &mut rows.edit,
            "Mode",
            &mut ae,
            camera.isp.ae_mode,
            &AE_MODES,
        );
        if ae != before {
            set_ae(rows.isp, ae);
        }
        let ae = ae.unwrap_or(camera.isp.ae_mode);
        if ae != AeMode::Fixed {
            rows.field(ui, Field::AeTarget, "Target");
        }
        if ae == AeMode::Servo {
            rows.field(ui, Field::AeTau, "Servo lag");
        }
    });
    if rows
        .isp
        .as_ref()
        .is_some_and(|isp| isp.xyz_to_cam.is_some())
    {
        panels::hint(
            ui,
            "The preset's colour-matrix override is kept as written.",
        );
    }

    if scroll == Some(PanelId::Jpeg) {
        ui.scroll_to_cursor(Some(eframe::egui::Align::TOP));
    }
    jpeg(ui, camera, mode);
    rows.edit
}

/// The JPEG encoder's settings for the readout mode, read-only.
fn jpeg(ui: &mut Ui, camera: &Camera, mode: Mode) {
    panels::section(ui, "JPEG encoder");
    let quality = match mode {
        Mode::Still => camera.still.jpeg,
        Mode::Movie => camera.movie.jpeg,
    };
    panels::grid(ui, "isp-jpeg", |ui| {
        let quality = match quality {
            Quality::Superfine => "Superfine",
            Quality::Fine => "Fine",
            Quality::Normal => "Normal",
        };
        panels::value_row(
            ui,
            "Quality",
            quality,
            "The camera's quantisation tables for the readout mode's quality setting",
        );
        let subsampling = match camera.jpeg.subsampling {
            Subsampling::S444 => "4:4:4",
            Subsampling::S422 => "4:2:2",
            Subsampling::S420 => "4:2:0",
        };
        let upsampling = match camera.jpeg.chroma_upsample {
            ChromaUp::Nearest => "nearest",
            ChromaUp::Bilinear => "bilinear",
        };
        panels::value_row(
            ui,
            "Chroma",
            &format!("{subsampling}, {upsampling} upsampling"),
            "Chroma subsampling of the encoder and upsampling of the decoder that shows the file",
        );
    });
}

/// What the processor panel's rows share: the overrides, the camera and the render width.
struct Rows<'a> {
    isp: &'a mut Option<IspOverride>,
    camera: &'a Camera,
    width: f64,
    edit: Edit,
}

impl Rows<'_> {
    /// A grid row overriding `field`, through the camera's value while untouched.
    fn field(&mut self, ui: &mut Ui, field: Field, label: &str) {
        let (shown, touched) = value(self.isp.as_ref(), self.camera, field);
        let mut slot = touched.then_some(shown);
        let before = slot;
        panels::override_row(
            ui,
            &mut self.edit,
            label,
            hover(field),
            &mut slot,
            field.camera(self.camera),
            &field.knob(self.width),
        );
        if slot != before {
            match slot {
                Some(value) => set(self.isp, field, value),
                None => reset(self.isp, field),
            }
        }
    }
}

/// The tooltip of a field.
fn hover(field: Field) -> &'static str {
    match field {
        Field::WbRedGain => {
            "Daylight gain of the red channel relative to green; the servo starts here"
        }
        Field::WbBlueGain => {
            "Daylight gain of the blue channel relative to green; the servo starts here"
        }
        Field::WbTau => "Time constant of the white-balance servo, in seconds of camera time",
        Field::ToneContrast => "Slope of the tone curve at mid-grey",
        Field::ToneToe => {
            "Linear level below which the tone curve compresses shadows towards black"
        }
        Field::ToneShoulder => {
            "Part of the output range below white given to the highlight roll-off"
        }
        Field::ChromaNr => "Blur of the colour-difference planes: its standard deviation in pixels",
        Field::LumaNr => "Blur of luma in flat areas: its standard deviation in pixels",
        Field::SharpenRadius => "Blur of the unsharp mask: its standard deviation in pixels",
        Field::SharpenGain => "Fraction of the high-pass detail the unsharp mask adds back",
        Field::AeTarget => "Mean luminance auto-exposure aims for, in percent of reference white",
        Field::AeTau => "Time constant of the auto-exposure servo, in seconds of camera time",
    }
}
