//! Post-processing of the camera's JPEG output, outside the simulated hardware.
//!
//! Every look in this crate comes from the physics of the signal chain. A post module
//! is admitted only with a written justification in `docs/POST-STAGE.md` answering
//! three questions: which look it produces, why no stage of the signal chain can
//! produce it physically, and what it operates on. Post modules operate on the
//! camera's output [`Frame`](crate::Frame) only, never on intermediate signals.
//!
//! Colour comes from the signal chain, never from palette lookup tables. No post
//! module is currently admitted.
