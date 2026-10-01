//! Which App surfaces may inherit a local Computer Use grant.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComputerUseSurface {
    LocalInteractive,
    RemoteIm,
    Scheduled,
    Ssh,
}

pub fn surface_allows_local(surface: ComputerUseSurface) -> bool {
    matches!(surface, ComputerUseSurface::LocalInteractive)
}

pub fn classify_surface(scheduled: bool, ssh: bool, remote_im: bool) -> ComputerUseSurface {
    if remote_im {
        ComputerUseSurface::RemoteIm
    } else if scheduled {
        ComputerUseSurface::Scheduled
    } else if ssh {
        ComputerUseSurface::Ssh
    } else {
        ComputerUseSurface::LocalInteractive
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_im_scheduler_ssh_cannot_use_local_computer_use() {
        assert!(surface_allows_local(ComputerUseSurface::LocalInteractive));
        assert!(!surface_allows_local(ComputerUseSurface::RemoteIm));
        assert!(!surface_allows_local(ComputerUseSurface::Scheduled));
        assert!(!surface_allows_local(ComputerUseSurface::Ssh));
        assert_eq!(
            classify_surface(false, false, true),
            ComputerUseSurface::RemoteIm
        );
        assert_eq!(
            classify_surface(true, true, false),
            ComputerUseSurface::Scheduled
        );
        assert_eq!(
            classify_surface(false, true, false),
            ComputerUseSurface::Ssh
        );
        assert_eq!(
            classify_surface(false, false, false),
            ComputerUseSurface::LocalInteractive
        );
    }
}
