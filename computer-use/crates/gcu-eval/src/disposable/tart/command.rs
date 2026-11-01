//! Fixed Tart operations. There is no argv string and no shell string.

use super::identity::{CloneId, GoldenImage};
use super::policy::SoftnetFlags;

/// One Tart operation the runner can record.
///
/// Variants are the whole command. Nothing here is executed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TartCommand {
    /// `clone` of the fixed golden image to one reserved clone id.
    Clone {
        /// Golden image. Not a path.
        golden: GoldenImage,
        /// Destination id from the ledger permit.
        clone: CloneId,
    },
    /// `start` with the required Softnet flags. No unrestricted start exists.
    Start {
        /// Clone to start.
        clone: CloneId,
        /// Structured flags. The block-all entry is always present.
        softnet: SoftnetFlags,
    },
    /// `stop` of one clone id.
    Stop {
        /// Clone to stop.
        clone: CloneId,
    },
    /// `delete` of one clone id. A golden name cannot be built as a [`CloneId`].
    Delete {
        /// Clone to delete.
        clone: CloneId,
    },
    /// `list` with no filter, so it cannot become a prefix delete.
    List,
    /// `status` of one exact clone id.
    Status {
        /// Clone to query.
        clone: CloneId,
    },
}
