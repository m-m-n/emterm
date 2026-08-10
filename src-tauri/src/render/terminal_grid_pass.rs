//! Custom wgpu render pass that paints the terminal grid.
//!
//! Phase 4-H of font-swash-migration (FR12). This pass is the sole consumer
//! of the two-region font atlas (Alpha R8 + RGBA8) and the glyph cache. It
//! emits one instanced quad per non-empty cell + one instanced quad per
//! background fill / decoration line. The shader branches on
//! `atlas_page_kind` so monochrome glyphs get foreground-color modulation
//! and color glyphs (Noto Color Emoji CBDT / COLR v1) are sampled as-is.
//!
//! The frame draw order managed by `window_host::render` is
//! `clear -> TerminalGridPass -> egui (LoadOp::Load)`.
//! egui therefore retains the UI overlay only (tab bar / status bar /
//! IME preedit / settings panel); it no longer draws cell glyphs.
//!
//! Implementation strategy: pipeline + bind group layout + per-frame
//! instance buffer + a tiny local `bytemuck`-style cast helper so we do
//! not add a new dependency.

use std::sync::Arc;

use parking_lot::Mutex;

use super::font::cache::GlyphCache;
use super::font::fallback::FallbackChain;
use super::font::traits::GlyphRasterizer;

/// Page index encoded into each instance for the WGSL shader. 0 == Alpha
/// (R8, modulated by fg), 1 == Rgba (RGBA8, sampled as-is), 2 == solid
/// fill (no atlas read; used for background quads + decoration lines),
/// 3 == Subpixel (RGBA8 coverage mask on the RGBA page; per-channel
/// fg/bg blend in the shader — LCD anti-aliasing).
const PAGE_ALPHA: u32 = 0;
const PAGE_RGBA: u32 = 1;
const PAGE_SOLID: u32 = 2;
const PAGE_SUBPIXEL: u32 = 3;

/// Decoration bit flags packed into the instance `flags` field.
const FLAG_UNDERLINE: u32 = 1 << 0;
const FLAG_STRIKETHROUGH: u32 = 1 << 1;
/// Solid-page fg-color fill (procedural box-drawing strokes, block
/// elements, shade alpha-blends). Without this flag a `PAGE_SOLID`
/// instance falls into the background-fill branch and renders the
/// cell's bg color — i.e. invisible.
const FLAG_FG_FILL: u32 = 1 << 2;

const SHADER_SRC: &str = include_str!("terminal_grid_pass.wgsl");

/// Per-instance vertex layout matching the WGSL `Instance` struct.
///
/// `cell_xy` / `cell_wh` are in pixel space (clip-space conversion happens
/// in the vertex shader via the `viewport` uniform). `atlas_uv` is the
/// `(u0, v0, u1, v1)` rect inside the active atlas page; for `PAGE_SOLID`
/// instances it is ignored. Colors are packed RGBA8 as a single `u32` to
/// keep the instance stride small.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct CellInstance {
    pub cell_xy: [f32; 2],
    pub cell_wh: [f32; 2],
    pub atlas_uv: [f32; 4],
    pub fg_rgba: u32,
    pub bg_rgba: u32,
    pub page: u32,
    pub flags: u32,
}

impl CellInstance {
    pub const STRIDE: u64 = std::mem::size_of::<Self>() as u64;
}

unsafe impl bytemuck_compat::Pod for CellInstance {}

/// Global uniform: swapchain viewport in pixels + atlas page sizes (used to
/// turn the integer atlas region into normalized UV coordinates inside the
/// vertex shader) + decoration line thickness. `decoration_thickness_px` is
/// the **single source of truth** for SGR underline / strikethrough band
/// thickness — computed on the CPU with `f32::round()` from the same
/// `metrics.cell_h` that `box_drawing::rects_for` consumes, so SGR
/// underline and `─` (U+2500) end up at exactly the same pixel weight on
/// screen regardless of HiDPI scale or font size. Avoids duplicating the
/// `cell_h / 18` formula in the shader and the WGSL `round()` (ties-to-
/// even) vs Rust `f32::round()` (ties-away) tie-break divergence.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
struct FrameUniform {
    viewport: [f32; 2],
    alpha_atlas: [f32; 2],
    rgba_atlas: [f32; 2],
    decoration_thickness_px: f32,
    _pad: f32,
}

unsafe impl bytemuck_compat::Pod for FrameUniform {}

/// SGR underline / strikethrough band thickness in physical pixels.
/// Funneled through `box_drawing::light_stroke_px` so the SGR decoration
/// and procedural box-drawing strokes are guaranteed to match weight.
fn decoration_thickness_px(cell_h: f32) -> f32 {
    super::box_drawing::light_stroke_px(cell_h)
}

mod bytemuck_compat {
    /// # Safety
    /// Implementors guarantee a defined `#[repr(C)]` byte representation.
    pub unsafe trait Pod: Copy + 'static {}

    pub fn cast_slice<T: Pod>(slice: &[T]) -> &[u8] {
        let len_bytes = std::mem::size_of_val(slice);
        // SAFETY: `Pod` implementors are safe to reinterpret as bytes.
        unsafe { std::slice::from_raw_parts(slice.as_ptr() as *const u8, len_bytes) }
    }
}

/// Shrink-to-fit policy for a cell's glyph quad.
///
/// Ordinary cells use [`GlyphFit::HorizontalOnly`] to fix the
/// ambiguous-width-rendering SPEC's FR2 — a Dingbat / Symbol glyph
/// rendered from a CJK fallback whose design advance is ~1.5 em is
/// shrunk horizontally so its bitmap stops bleeding into the next
/// cell, while a Latin monospace glyph (advance == cell_w) sees
/// `sx = 1.0` and isn't crushed by its natural AA overhang. IME
/// preedit overlay uses [`GlyphFit::Both`] so CJK descenders past
/// `cell_h` are additionally clamped inside the reverse-video bg.
/// [`GlyphFit::None`] preserves natural metrics in both axes.
///
/// Replacing the prior `fit_glyph_to_cell` + `fit_glyph_vertical`
/// boolean pair: the four-combination matrix included one dead state
/// (`(false, true)` — vertical fit without horizontal) that no
/// caller wants but a typo could produce. The enum makes the three
/// meaningful states exhaustive and the dead one unrepresentable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlyphFit {
    /// Render the glyph at its natural metrics. Used by ordinary
    /// cells under the previous renderer (pre-FR2 port) and by tests
    /// that don't exercise the fit path.
    None,
    /// Shrink horizontally so the glyph advance fits the cell
    /// footprint; leave the vertical axis at natural metrics so
    /// descenders / ascenders aren't visibly crushed.
    HorizontalOnly,
    /// Shrink both axes so the entire bitmap quad fits the cell rect.
    /// Used by the IME preedit overlay so reverse-video bg contains
    /// the full glyph including CJK descenders.
    Both,
}

impl GlyphFit {
    /// True when horizontal shrink-to-fit applies. Both `HorizontalOnly`
    /// and `Both` opt in; `None` does not.
    pub fn horizontal(self) -> bool {
        matches!(self, GlyphFit::HorizontalOnly | GlyphFit::Both)
    }
    /// True only for `Both`. `HorizontalOnly` does NOT touch the
    /// vertical axis.
    pub fn vertical(self) -> bool {
        matches!(self, GlyphFit::Both)
    }
}

/// Per-cell input to [`TerminalGridPass::prepare`].
///
/// `glyph` is the grapheme cluster string. Empty / single-space clusters
/// emit only the background quad (no glyph instance). `fg_rgba` / `bg_rgba`
/// are little-endian RGBA8 packs (`[r, g, b, a]` in that order).
#[derive(Debug, Clone)]
pub struct CellInput {
    pub col: u16,
    pub row: u16,
    pub width_cells: u8,
    pub glyph: String,
    pub fg_rgba: [u8; 4],
    pub bg_rgba: [u8; 4],
    pub underline: bool,
    pub strikethrough: bool,
    pub draw_background: bool,
    /// Extra height (in logical pixels, scaled by the same factor the
    /// caller passed for `cell_h`) added to the bg quad below the cell
    /// rect. Used by the IME preedit overlay so a reverse-video bg
    /// covers CJK glyph descenders that naturally rasterize past
    /// `cell_h`. `0.0` for ordinary cells.
    pub bg_extend_below: f32,
    /// Shrink policy for the glyph quad when its natural bitmap
    /// exceeds the cell footprint. See [`GlyphFit`] for the variant
    /// semantics. Replaces the prior pair of `(fit_glyph_to_cell,
    /// fit_glyph_vertical)` bools — the four-combination matrix
    /// included one dead state (`vertical only`), and the enum makes
    /// the three meaningful modes match-exhaustive at the consumer
    /// site.
    pub fit: GlyphFit,
    /// SGR bold: render the glyph with the resolved font's bold face
    /// when one is registered on the fallback chain (see
    /// `FallbackChain::bold_variant`). Fonts without a bold variant
    /// keep their regular face.
    pub bold: bool,
}

/// Cell metrics used by [`TerminalGridPass::prepare`] when converting
/// `(col, row)` to pixel rects. Decoration line offsets are derived inside
/// the WGSL shader from the cell rect.
#[derive(Debug, Clone, Copy)]
pub struct CellMetrics {
    pub cell_w: f32,
    pub cell_h: f32,
    pub origin: [f32; 2],
    pub font_size_px: f32,
}

mod builder;
use builder::*;

/// Custom wgpu pass that draws the entire terminal grid in one instanced
/// draw call.
///
/// The pass owns the pipeline + bind-group layout + sampler + persistent
/// GPU buffers. It does NOT own the glyph cache or atlas — those live
/// alongside the renderer so they can be reused across frames (see
/// [`GridInstanceBuilder`], which the CPU-side glyph shaping + row-cache
/// logic now lives on). `prepare` uploads an already-resolved instance
/// list (grown/updated in place rather than reallocated every frame —
/// task0003 AC-4); `draw` records the instanced draw call into a render
/// pass started with `LoadOp::Load` (so the wgpu clear performed before
/// this pass survives).
pub struct TerminalGridPass {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// Lazily-uploaded textures. Replaced on every `prepare` when the atlas
    /// page bytes change.
    alpha_texture: Option<wgpu::Texture>,
    alpha_view: Option<wgpu::TextureView>,
    alpha_dim: (u32, u32),
    rgba_texture: Option<wgpu::Texture>,
    rgba_view: Option<wgpu::TextureView>,
    rgba_dim: (u32, u32),
    /// CPU-side glyph shaping + per-row instance cache.
    builder: GridInstanceBuilder,
    /// Persistent GPU-side instance buffer (task0003 AC-4): created once
    /// and grown via [`grow_capacity`] instead of reallocated every frame.
    instance_buffer: Option<wgpu::Buffer>,
    /// Capacity of `instance_buffer` in bytes.
    instance_capacity_bytes: u64,
    /// Persistent GPU-side uniform buffer. Fixed size
    /// (`size_of::<FrameUniform>()`), so it is created once and only ever
    /// `write_buffer`'d in place afterward.
    uniform_buffer: Option<wgpu::Buffer>,
    /// Bind group referencing `uniform_buffer` + the atlas texture views +
    /// sampler. Rebuilt only when a referenced resource's identity changes
    /// (first creation, or atlas texture (re)creation).
    bind_group: Option<wgpu::BindGroup>,
    /// Instance count uploaded this frame; `draw` reads this instead of a
    /// per-call parameter now that the instance buffer itself is
    /// persistent.
    instance_count: usize,
    /// The atlas content generation that the GPU textures currently reflect.
    /// `None` until the first upload has been performed.
    uploaded_generation: Option<u64>,
}

impl TerminalGridPass {
    /// Build the pipeline + bind group layout. The atlas textures are
    /// uploaded lazily on the first `prepare` call (the atlas page sizes
    /// are not known until the cache has uploaded at least one glyph).
    pub fn new(
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
        cache: Arc<Mutex<GlyphCache>>,
        fallback: Arc<FallbackChain>,
        rasterizer: Arc<dyn GlyphRasterizer>,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("native-poc-terminal-grid-shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER_SRC.into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("native-poc-terminal-grid-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("native-poc-terminal-grid-pl"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        // Instance buffer layout. Eight scalar attributes packed as
        // `vec2<f32>`, `vec2<f32>`, `vec4<f32>`, four `u32`s.
        let attributes = [
            // cell_xy
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x2,
                offset: 0,
                shader_location: 0,
            },
            // cell_wh
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x2,
                offset: 8,
                shader_location: 1,
            },
            // atlas_uv (u0,v0,u1,v1)
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Float32x4,
                offset: 16,
                shader_location: 2,
            },
            // fg_rgba
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Uint32,
                offset: 32,
                shader_location: 3,
            },
            // bg_rgba
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Uint32,
                offset: 36,
                shader_location: 4,
            },
            // page
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Uint32,
                offset: 40,
                shader_location: 5,
            },
            // flags
            wgpu::VertexAttribute {
                format: wgpu::VertexFormat::Uint32,
                offset: 44,
                shader_location: 6,
            },
        ];

        let vbuf_layout = wgpu::VertexBufferLayout {
            array_stride: CellInstance::STRIDE,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &attributes,
        };

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("native-poc-terminal-grid-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[vbuf_layout],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::SrcAlpha,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("native-poc-terminal-grid-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        Self {
            pipeline,
            bind_group_layout,
            sampler,
            alpha_texture: None,
            alpha_view: None,
            alpha_dim: (0, 0),
            rgba_texture: None,
            rgba_view: None,
            rgba_dim: (0, 0),
            builder: GridInstanceBuilder::new(cache, fallback, rasterizer),
            instance_buffer: None,
            instance_capacity_bytes: 0,
            uniform_buffer: None,
            bind_group: None,
            instance_count: 0,
            uploaded_generation: None,
        }
    }

    /// CPU-side build path (no GPU): delegates to
    /// [`GridInstanceBuilder::build_instances`]. Used directly by the IME
    /// preedit bypass path (task0003 D3) — a frame with active preedit
    /// rebuilds the full grid fresh rather than going through the per-row
    /// cache. Also exercised by this module's device-free tests.
    ///
    /// This split exists so unit tests can exercise the per-cell pipeline
    /// (TS-font-13 / TS-font-14) without standing up a wgpu device.
    pub fn build_instances(&self, cells: &[CellInput], metrics: CellMetrics) -> Vec<CellInstance> {
        self.builder.build_instances(cells, metrics)
    }

    /// CPU-side entry point for the cached (non-preedit) render path
    /// (task0003 FR3/FR4): delegates to
    /// [`GridInstanceBuilder::rebuild_and_collect`]. Rebuilds exactly
    /// `dirty_rows` from `dirty_cells` and returns `(instances,
    /// rows_rebuilt)` — `window_host::render` feeds `rows_rebuilt` into
    /// the `EMTERM_RENDER_PERF` rows-rebuilt counter.
    pub fn rebuild_and_collect(
        &mut self,
        dirty_rows: &[u16],
        dirty_cells: &[CellInput],
        metrics: CellMetrics,
        row_count: u16,
    ) -> (Vec<CellInstance>, usize) {
        self.builder
            .rebuild_and_collect(dirty_rows, dirty_cells, metrics, row_count)
    }

    /// CPU-side entry point (task0006): consume term_core's accumulated
    /// scroll event by rotating the per-row cache. See
    /// [`GridInstanceBuilder::apply_scroll_event`] /
    /// [`RowCache::rotate_for_scroll_event`] for the rotation semantics.
    /// Callers read `direction` / `count` from
    /// `TerminalCore::get_scroll_event_direction()` /
    /// `get_scroll_event_count()` and clear the core-side event
    /// afterward (`TerminalCore::clear_scroll_event()`) — this method
    /// only touches the renderer-side cache, once per rendered frame,
    /// before the dirty-row rebuild. `cell_h` must match the
    /// `CellMetrics` used for this frame's rebuild.
    pub fn apply_scroll_event(&mut self, direction: u8, count: u16, cell_h: f32) {
        self.builder.apply_scroll_event(direction, count, cell_h);
    }

    /// Upload this frame's already-resolved instance list to the GPU and
    /// (re)build the bind group as needed. Callers resolve `instances`
    /// beforehand — either via [`Self::rebuild_and_collect`] (the cached
    /// path) or [`Self::build_instances`] (the IME preedit bypass / any
    /// other full-grid path) — so this method is pure GPU plumbing: atlas
    /// texture sync, persistent instance/uniform buffer management (grown
    /// via [`grow_capacity`] instead of reallocated every frame — task0003
    /// AC-4), and bind-group (re)creation. Called once per frame from
    /// `window_host::render`.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        instances: &[CellInstance],
        metrics: CellMetrics,
        viewport_w: u32,
        viewport_h: u32,
    ) {
        self.instance_count = instances.len();
        if instances.is_empty() {
            return;
        }
        // Sync the GPU atlas textures with the CPU atlas bytes.
        let (alpha_dim, rgba_dim, generation) = {
            let cache = self.builder.cache.lock();
            (
                cache.atlas().alpha_dim(),
                cache.atlas().rgba_dim(),
                cache.atlas().generation(),
            )
        };
        // Track whether either texture was (re)created this call. A freshly
        // created texture has undefined/zeroed contents and must be uploaded
        // regardless of the atlas generation counter.
        let mut texture_recreated = false;
        if Some(alpha_dim) != Some(self.alpha_dim) || self.alpha_texture.is_none() {
            self.alpha_dim = alpha_dim;
            let tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("native-poc-terminal-grid-alpha-atlas"),
                size: wgpu::Extent3d {
                    width: alpha_dim.0.max(1),
                    height: alpha_dim.1.max(1),
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            self.alpha_view = Some(tex.create_view(&wgpu::TextureViewDescriptor::default()));
            self.alpha_texture = Some(tex);
            texture_recreated = true;
        }
        if Some(rgba_dim) != Some(self.rgba_dim) || self.rgba_texture.is_none() {
            self.rgba_dim = rgba_dim;
            let tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("native-poc-terminal-grid-rgba-atlas"),
                size: wgpu::Extent3d {
                    width: rgba_dim.0.max(1),
                    height: rgba_dim.1.max(1),
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                // Non-sRGB on purpose: the atlas holds sRGB-encoded
                // premultiplied bytes and the surface is non-sRGB, so the
                // bytes must pass through sampling un-decoded to land on
                // screen verbatim (gamma-space pipeline, matching the
                // WebView build).
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            self.rgba_view = Some(tex.create_view(&wgpu::TextureViewDescriptor::default()));
            self.rgba_texture = Some(tex);
            texture_recreated = true;
        }
        // Upload atlas pages only when the atlas content generation advanced
        // (new glyphs were rasterized) or a texture was (re)created this call.
        // Steady-state frames pay zero atlas upload bandwidth — this matters
        // because subpixel masks moved common text glyphs onto the 4-byte-per-
        // pixel RGBA page, making unconditional uploads expensive.
        let needs_upload = self.uploaded_generation != Some(generation) || texture_recreated;
        if needs_upload {
            let cache = self.builder.cache.lock();
            if let Some(tex) = self.alpha_texture.as_ref() {
                queue.write_texture(
                    wgpu::ImageCopyTexture {
                        texture: tex,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    cache.atlas().alpha_bytes(),
                    wgpu::ImageDataLayout {
                        offset: 0,
                        bytes_per_row: Some(alpha_dim.0),
                        rows_per_image: Some(alpha_dim.1),
                    },
                    wgpu::Extent3d {
                        width: alpha_dim.0,
                        height: alpha_dim.1,
                        depth_or_array_layers: 1,
                    },
                );
            }
            if let Some(tex) = self.rgba_texture.as_ref() {
                queue.write_texture(
                    wgpu::ImageCopyTexture {
                        texture: tex,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    cache.atlas().rgba_bytes(),
                    wgpu::ImageDataLayout {
                        offset: 0,
                        bytes_per_row: Some(rgba_dim.0 * 4),
                        rows_per_image: Some(rgba_dim.1),
                    },
                    wgpu::Extent3d {
                        width: rgba_dim.0,
                        height: rgba_dim.1,
                        depth_or_array_layers: 1,
                    },
                );
            }
            self.uploaded_generation = Some(generation);
        }

        let uniform = FrameUniform {
            viewport: [viewport_w as f32, viewport_h as f32],
            alpha_atlas: [alpha_dim.0 as f32, alpha_dim.1 as f32],
            rgba_atlas: [rgba_dim.0 as f32, rgba_dim.1 as f32],
            decoration_thickness_px: decoration_thickness_px(metrics.cell_h),
            _pad: 0.0,
        };
        // Persistent uniform buffer (task0003 AC-4): fixed size, so it is
        // only ever created once (first call) and `write_buffer`'d in
        // place on every subsequent call.
        let uniform_first_created = self.uniform_buffer.is_none();
        let uniform_buffer = self.uniform_buffer.get_or_insert_with(|| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("native-poc-terminal-grid-uniform"),
                size: std::mem::size_of::<FrameUniform>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        });
        queue.write_buffer(uniform_buffer, 0, bytemuck_compat::cast_slice(&[uniform]));

        // Persistent instance buffer (task0003 AC-4): grown via
        // `grow_capacity` only when the required upload size exceeds the
        // current capacity; otherwise the existing buffer is reused and
        // just `write_buffer`'d in place, so a steady-state frame (same
        // instance count) allocates no new GPU buffer at all.
        let instance_bytes = bytemuck_compat::cast_slice(instances);
        let required = instance_bytes.len() as u64;
        let new_capacity = grow_capacity(self.instance_capacity_bytes, required);
        if self.instance_buffer.is_none() || new_capacity != self.instance_capacity_bytes {
            self.instance_buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("native-poc-terminal-grid-instances"),
                size: new_capacity,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
            self.instance_capacity_bytes = new_capacity;
        }
        queue.write_buffer(
            self.instance_buffer.as_ref().expect("just ensured above"),
            0,
            instance_bytes,
        );

        // Bind group references the uniform buffer (fixed identity once
        // created) + the atlas texture views — NOT the instance buffer
        // (bound separately via `set_vertex_buffer`), so instance-buffer
        // regrowth alone never requires a bind-group rebuild.
        if uniform_first_created || texture_recreated || self.bind_group.is_none() {
            self.bind_group = Some(
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("native-poc-terminal-grid-bg"),
                    layout: &self.bind_group_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: self
                                .uniform_buffer
                                .as_ref()
                                .expect("just ensured above")
                                .as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(
                                self.alpha_view
                                    .as_ref()
                                    .expect("alpha view present after upload"),
                            ),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::TextureView(
                                self.rgba_view
                                    .as_ref()
                                    .expect("rgba view present after upload"),
                            ),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: wgpu::BindingResource::Sampler(&self.sampler),
                        },
                    ],
                }),
            );
        }
    }

    /// Issue one instanced draw call. The render pass must already be
    /// configured with `LoadOp::Load` (`clear` ran in an earlier pass).
    /// Reads the persistent instance buffer / bind group / instance count
    /// [`Self::prepare`] populated this frame — a no-op when there is
    /// nothing to draw (no tab, or the last `prepare` saw zero instances).
    pub fn draw<'pass>(&'pass self, rpass: &mut wgpu::RenderPass<'pass>) {
        if self.instance_count == 0 {
            return;
        }
        let (Some(buf), Some(bg)) = (self.instance_buffer.as_ref(), self.bind_group.as_ref())
        else {
            return;
        };
        rpass.set_pipeline(&self.pipeline);
        rpass.set_bind_group(0, bg, &[]);
        rpass.set_vertex_buffer(0, buf.slice(..));
        rpass.draw(0..4, 0..self.instance_count as u32);
    }
}

/// Pack `[r, g, b, a]` (each 0..=255) into a little-endian `u32` so the
/// shader can unpack it via `unpack4x8unorm`.
fn pack_rgba(rgba: [u8; 4]) -> u32 {
    (rgba[3] as u32) << 24 | (rgba[2] as u32) << 16 | (rgba[1] as u32) << 8 | (rgba[0] as u32)
}

/// Clip a glyph quad horizontally to `[cell_left, cell_right]`, trimming
/// the atlas UV range proportionally so the remaining quad keeps its 1:1
/// texel mapping. Returns `None` when nothing of the quad survives.
///
/// Used by the subpixel path only: the subpixel fragment shader writes
/// `fg*mask + bg*(1-mask)` opaquely across the whole quad, so a quad
/// overhanging its cell would paint the cell's bg color outside the
/// cell. Quads that already fit pass through unchanged.
fn clip_quad_to_cell_x(
    glyph_x: f32,
    glyph_w: f32,
    u0: f32,
    u1: f32,
    cell_left: f32,
    cell_right: f32,
) -> Option<(f32, f32, f32, f32)> {
    if glyph_w <= 0.0 {
        return None;
    }
    let texels_per_px = (u1 - u0) / glyph_w;
    let mut x = glyph_x;
    let mut w = glyph_w;
    let mut nu0 = u0;
    let mut nu1 = u1;
    let left_trim = cell_left - x;
    if left_trim > 0.0 {
        nu0 += left_trim * texels_per_px;
        x += left_trim;
        w -= left_trim;
    }
    let right_trim = (x + w) - cell_right;
    if right_trim > 0.0 {
        nu1 -= right_trim * texels_per_px;
        w -= right_trim;
    }
    if w <= 0.0 {
        return None;
    }
    Some((x, w, nu0, nu1))
}

/// Y-axis twin of [`clip_quad_to_cell_x`]. Same shaving math, vertical
/// orientation: trims a glyph quad to the cell's [top, bottom] bounds and
/// shifts the V coordinates so the visible portion still maps 1:1 to its
/// atlas texels.
///
/// Used by the subpixel path to prevent a tall glyph (U+25FB ◻ from Noto
/// Sans Symbols 2, CJK descenders past the cell descent) from painting
/// this cell's bg color into the row above / below as a coloured stripe.
fn clip_quad_to_cell_y(
    glyph_y: f32,
    glyph_h: f32,
    v0: f32,
    v1: f32,
    cell_top: f32,
    cell_bottom: f32,
) -> Option<(f32, f32, f32, f32)> {
    if glyph_h <= 0.0 {
        return None;
    }
    let texels_per_px = (v1 - v0) / glyph_h;
    let mut y = glyph_y;
    let mut h = glyph_h;
    let mut nv0 = v0;
    let mut nv1 = v1;
    let top_trim = cell_top - y;
    if top_trim > 0.0 {
        nv0 += top_trim * texels_per_px;
        y += top_trim;
        h -= top_trim;
    }
    let bottom_trim = (y + h) - cell_bottom;
    if bottom_trim > 0.0 {
        nv1 -= bottom_trim * texels_per_px;
        h -= bottom_trim;
    }
    if h <= 0.0 {
        return None;
    }
    Some((y, h, nv0, nv1))
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod gpu_tests;
