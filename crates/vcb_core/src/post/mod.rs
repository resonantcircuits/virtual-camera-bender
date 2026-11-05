//! Post-processing of the camera's JPEG output, outside the simulated hardware.
//!
//! Every look in this crate comes from the physics of the signal chain. A post module
//! is admitted only with an entry in `docs/POST-STAGE.md` answering the four questions
//! listed there: which look it produces, why no stage of the signal chain can produce
//! it physically, what it takes and returns, and in which physical or workflow units
//! its parameters are given. Post modules take the camera's 8-bit output
//! [`Frame`](crate::Frame) and return a frame of the same size, never reading
//! intermediate signals.
//!
//! Colour comes from the signal chain, never from palette lookup tables. No post
//! module is currently admitted.
