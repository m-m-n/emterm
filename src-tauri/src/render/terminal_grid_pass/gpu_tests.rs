//! Tests that require a wgpu device. They are kept off by default
//! because Linux Docker test runs in this repo do not provision a
//! GPU; they are exercised by hand on a host with a real adapter.
use super::*;

/// TS-font-int-4: `TerminalGridPass` builds against the wgpu device
/// used by `window_host` (smoke pipeline-build test). Skipped on
/// hosts without a working adapter (returns Ok without asserting).
#[test]
fn pipeline_builds_against_wgpu_device() {
    // Try to obtain a wgpu device. On hosts without a GPU adapter
    // (the Docker e2e container is typically headless) `request_adapter`
    // returns None — we treat that as a skip rather than a failure so
    // the test suite stays green in CI.
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::all(),
        ..Default::default()
    });
    let adapter = match pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::default(),
        force_fallback_adapter: true,
        compatible_surface: None,
    })) {
        Some(a) => a,
        None => {
            eprintln!("skipping TS-font-int-4: no wgpu adapter available");
            return;
        }
    };
    let (device, _queue) = match pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: Some("ts-font-int-4-device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults(),
            memory_hints: wgpu::MemoryHints::Performance,
        },
        None,
    )) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("skipping TS-font-int-4: device request failed: {e}");
            return;
        }
    };

    // Standard stack: fallback chain rooted at a sentinel id, swash
    // rasterizer fed by the bundled fonts.
    let mut resolver = super::super::font::resolver::Resolver::new();
    let (cjk, _emoji, _mono, _base, _sym) = resolver.register_bundled();
    let swash = Arc::new(super::super::font::swash_adapter::SwashRasterizer::new());
    swash.ingest_resolver(&resolver);
    let chain = Arc::new(FallbackChain::new(cjk, []));
    let cache = Arc::new(Mutex::new(GlyphCache::new()));
    let _pass = TerminalGridPass::new(
        &device,
        wgpu::TextureFormat::Bgra8Unorm,
        cache,
        chain,
        swash as Arc<dyn GlyphRasterizer>,
    );
    // Reaching this line means pipeline + bind-group-layout creation
    // succeeded. No draw call needed for the smoke test.
}
