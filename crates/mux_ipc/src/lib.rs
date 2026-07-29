//! mux_ipc: shared wire protocol data types for emterm mux daemon.
//!
//! This crate hosts the protocol-only portion of the mux IPC stack so that
//! both the legacy Tauri build (`src-tauri`) and `native-poc` can speak the
//! same wire format without dragging server-side tokio_util glue across the
//! crate boundary. Server framing (`codec`, `connection`) intentionally
//! remains in `src-tauri/src/mux/ipc/`.
//!
//! Includes the mux agent-status / agent-API message additions
//! (`AgentStatusUpdate`, `ReadPane`/`ReadPaneResult`, `SendText`/
//! `SendTextResult`, `WaitAgentState`/`WaitAgentStateResult`,
//! `AgentApiError`) and the public pane ID helpers (`PublicPaneId`); see
//! `protocol` for the full type list.
//!
//! `handoff` adds the versioned handoff document type and the `Upgrade` /
//! `Upgrading` control messages used by the mux daemon hot-upgrade feature
//! (`protocol::MessageType::Upgrade` / `Upgrading`; `handoff::HandoffDocument`).

pub mod handoff;
pub mod protocol;
