//! Cooperative X11 capture lease ordering, independent of window APIs.
/// OS operations used by the X11 capture barrier. No driver/action mutexes.
pub trait CaptureSurface {
    /// Platform error.
    type Error;
    /// Hide every overlay and flush the server before returning success.
    fn hide_all(&mut self) -> Result<(), Self::Error>;
    /// Publish exact current lease nonces only after hiding completed.
    fn acknowledge(&mut self, proofs: &[[u32; 5]]) -> Result<(), Self::Error>;
    /// Show nonactivating surfaces only with no active capture lease.
    fn show_all(&mut self) -> Result<(), Self::Error>;
}
/// Process fresh capture leases. A failed hide cannot acknowledge pixels; an
/// existing lease always prevents remapping. Synchronous bounded OS work is
/// supplied by the adapter; cancellation/timeout belongs to the capture driver.
pub fn capture_barrier<S: CaptureSurface>(
    surface: &mut S,
    proofs: &[[u32; 5]],
) -> Result<(), S::Error> {
    if !proofs.is_empty() {
        surface.hide_all()?;
    }
    surface.acknowledge(proofs)?;
    if proofs.is_empty() {
        surface.show_all()?;
    }
    Ok(())
}
