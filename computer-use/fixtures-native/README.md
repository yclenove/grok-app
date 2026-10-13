# Native platform fixtures

P1-01 will add native fixtures where the shared egui + AccessKit fixture
cannot represent platform behavior: AppKit on macOS, Win32 or equivalent
Windows controls, and GTK4 on Linux. Fixtures must use synthetic data and
report outcomes independently of the computer-use engine.

This directory is documentation only until P1-01; it contains no empty
Cargo packages and is not a workspace member.
