//! Trusted Wayland adapter assembly, kept outside the driver trait implementation.
use super::*;

impl LinuxDriver {
    /// Access the detected Linux environment.
    pub fn environment(&self) -> &LinuxEnvironment {
        &self.env
    }

    /// Access the detected portal info.
    pub fn portal_info(&self) -> &PortalInfo {
        &self.portal_info
    }

    /// Override the detected Linux environment.
    pub fn with_environment(mut self, env: LinuxEnvironment) -> Self {
        self.env = env;
        self.recompute_capabilities();
        self
    }

    /// Override the detected portal capabilities.
    pub fn with_portal_info(mut self, portal: PortalInfo) -> Self {
        self.portal_info = portal;
        self.recompute_capabilities();
        self
    }

    /// Attach a fully consented, source-proven portal input session. Router policy still
    /// chooses delivery; this does not permit Auto/targeted routes to become foreground.
    pub fn with_portal_input(mut self, input: Arc<crate::wayland::input::PortalInput>) -> Self {
        self.portal_input = Some(input);
        self
    }

    /// Portal input adapter, if explicitly assembled by the trusted Host.
    pub fn portal_input(&self) -> Option<&Arc<crate::wayland::input::PortalInput>> {
        self.portal_input.as_ref()
    }
}
