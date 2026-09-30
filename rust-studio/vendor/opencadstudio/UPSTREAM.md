# OpenCADStudio UI source

Source: https://github.com/HakanSeven12/OpenCADStudio
Author/project: Hakan Seven and OpenCADStudio contributors.
Commit: 2a15a370f6886d3e8695ab81c7d677e0cd484451
License: GPL-3.0 (full upstream LICENSE retained).

The src/ui and assets/icons directories are an unmodified source snapshot.
The native app actively uses the SVG assets and ports ribbon rendering helpers
into crates/native-app/src/hakan_ui.rs. That port adapts the upstream Iced Git
API to Iced 0.14 and routes commands to the PDF document model.
The CAD-specific panels are retained here for subsequent migration, not yet wired.
