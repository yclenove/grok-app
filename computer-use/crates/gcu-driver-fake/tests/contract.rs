//! Portable model and driver contract regression tests.
#![cfg(feature = "testing")]
#![allow(clippy::unwrap_used, clippy::expect_used)]
gcu_core::contract!(gcu_driver_fake::FakeDriver::contract_fixture());
