# Threat Model: lib-budget-tests-load-tolerance

## Verdict
no-trust-boundary

## Rationale
Inspected SPEC.md and REQUIREMENTS.md (FR1-FR9, NFR1-NFR4) at the full tier, and the two task plans: task0001 (new test-side thread CPU-time helper, `src-tauri/Cargo.toml` feature, the helper's declaration in the pty_spawn tests module, `strip_concat_query.rs`) with domains concurrency and config-infra, and task0002 (`round4_chain.rs`) with no domain. No task declares auth, input-handling, external-io or data-persistence, so the analysis ran at standard depth.

Every changed Rust file is compiled only into the test build; no shipped binary gains code. The data handled is byte strings and workloads built inside the tests from constants, plus the calling thread's own CPU-time counter read from the operating system through the existing libc / windows-sys bindings. Nothing arrives from a user, the network, a file from outside the project, another process or an LLM prompt, and no privilege changes. The `src-tauri/Cargo.toml` change enables a feature of a dependency the crate already has; no new crate or supplier enters the build. No trust boundary is crossed.
