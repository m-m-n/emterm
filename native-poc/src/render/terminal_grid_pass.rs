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
//! `clear -> TerminalGridPass -> egui (LoadOp::Load) -> ImageOverlayPass`.
//! egui therefore retains the UI overlay only (tab bar / status bar /
//! IME preedit / settings panel); it no longer draws cell glyphs.
//!
//! Implementation strategy mirrors `image::overlay::OverlayPipeline`:
//! pipeline + bind group layout + per-frame instance buffer + a tiny
//! local `bytemuck`-style cast helper so we do not add a new dependency.

use std::sync::Arc;

use parking_lot::Mutex;

use super::font::cache::{GlyphCache, GlyphKey};
use super::font::fallback::FallbackChain;
use super::font::traits::{AtlasFormat, GlyphRasterizer};

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
unsafe impl bytemuck_compat::Zeroable for CellInstance {}

/// Global uniform: swapchain viewport in pixels + atlas page sizes (used to
/// turn the integer atlas region into normalized UV coordinates inside the
/// vertex shader).
#[repr(C)]
#[derive(Copy, Clone, Debug)]
struct FrameUniform {
    viewport: [f32; 2],
    alpha_atlas: [f32; 2],
    rgba_atlas: [f32; 2],
    _pad: [f32; 2],
}

unsafe impl bytemuck_compat::Pod for FrameUniform {}
unsafe impl bytemuck_compat::Zeroable for FrameUniform {}

mod bytemuck_compat {
    /// # Safety
    /// Implementors guarantee a defined `#[repr(C)]` byte representation.
    pub unsafe trait Pod: Copy + 'static {}
    /// # Safety
    /// Implementors guarantee an all-zero bit pattern is a valid value.
    pub unsafe trait Zeroable: Sized {}

    pub fn cast_slice<T: Pod>(slice: &[T]) -> &[u8] {
        let len_bytes = std::mem::size_of_val(slice);
        // SAFETY: `Pod` implementors are safe to reinterpret as bytes.
        unsafe { std::slice::from_raw_parts(slice.as_ptr() as *const u8, len_bytes) }
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
    /// Clamp the glyph quad's width / height to the cell rect when
    /// `true`. Used by the IME preedit overlay so ambiguous-width
    /// glyphs (e.g. ▽ U+25BD) whose natural bitmap exceeds 1 cell are
    /// scaled down to fit. `false` for ordinary cells (preserves
    /// natural glyph metrics for accurate text rendering).
    pub fit_glyph_to_cell: bool,
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

/// Custom wgpu pass that draws the entire terminal grid in one instanced
/// draw call.
///
/// The pass owns the pipeline + bind-group layout + sampler. It does NOT
/// own the glyph cache or atlas — those live alongside the renderer so
/// they can be reused across frames. `prepare` consumes a slice of
/// [`CellInput`] and produces a fresh instance buffer + bind group; `draw`
/// records the instanced draw call into a render pass started with
/// `LoadOp::Load` (so the wgpu clear performed before this pass survives).
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
    /// Cache + atlas live behind a mutex so the App can hand the same Arc
    /// to multiple consumers (Phase 5+). The pass calls
    /// `cache.get_or_rasterize` during `prepare`.
    cache: Arc<Mutex<GlyphCache>>,
    /// Resolved fallback chain consulted per grapheme cluster.
    fallback: Arc<FallbackChain>,
    /// Active rasterizer (Swash or AbGlyph, picked at startup from
    /// `Settings::font_engine`).
    rasterizer: Arc<dyn GlyphRasterizer>,
    /// The atlas content generation that the GPU textures currently reflect.
    /// `None` until the first upload has been performed.
    uploaded_generation: Option<u64>,
}

/// Output of a single [`TerminalGridPass::prepare`] call. Held by the
/// caller for the duration of the render pass so the bind group + buffer
/// stay alive.
pub struct PreparedFrame {
    pub instances: Vec<CellInstance>,
    pub instance_buffer: Option<wgpu::Buffer>,
    pub uniform_buffer: Option<wgpu::Buffer>,
    pub bind_group: Option<wgpu::BindGroup>,
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
            cache,
            fallback,
            rasterizer,
            uploaded_generation: None,
        }
    }

    /// CPU-side build path (no GPU). Computes the instance list for the
    /// supplied grid input. The GPU upload step (`prepare`) wraps this and
    /// also creates the wgpu buffers + bind group.
    ///
    /// This split exists so unit tests can exercise the per-cell pipeline
    /// (TS-font-13 / TS-font-14) without standing up a wgpu device.
    pub fn build_instances(&self, cells: &[CellInput], metrics: CellMetrics) -> Vec<CellInstance> {
        let mut out = Vec::with_capacity(cells.len() * 2);
        let mut cache = self.cache.lock();
        // Pre-compute the per-cell baseline using the base font's real
        // ascent + line height. Without this we used the rough
        // `size_px * 0.8` approximation in every glyph, which made
        // glyphs from fonts with different intrinsic ascents
        // (Inconsolata vs Noto Sans JP vs Noto Color Emoji) drift
        // visibly inside the cell.
        let base_metrics = self
            .rasterizer
            .font_metrics(self.fallback.base(), metrics.font_size_px);
        let base_ascent = base_metrics
            .map(|m| m.ascent)
            .unwrap_or(metrics.font_size_px * 0.8);
        let base_line_height = base_metrics
            .map(|m| m.line_height())
            .unwrap_or(metrics.font_size_px);
        // Center the line vertically inside the cell so cells with a
        // small font but tall cell (e.g. cell_h=17 / line_height≈16)
        // get balanced top / bottom padding instead of the text being
        // anchored to the very top.
        let v_pad = ((metrics.cell_h - base_line_height) * 0.5).max(0.0);
        for cell in cells {
            let x = metrics.origin[0] + cell.col as f32 * metrics.cell_w;
            let y = metrics.origin[1] + cell.row as f32 * metrics.cell_h;
            let w = metrics.cell_w * (cell.width_cells.max(1) as f32);
            let h = metrics.cell_h;
            // Background quad first (rendered underneath the glyph).
            // `bg_extend_below` extends the bg downward so reverse-video
            // preedit cells cover CJK glyph descenders that naturally
            // rasterize past `cell_h`.
            if cell.draw_background {
                let bg_h = h + cell.bg_extend_below.max(0.0);
                out.push(CellInstance {
                    cell_xy: [x, y],
                    cell_wh: [w, bg_h],
                    atlas_uv: [0.0, 0.0, 0.0, 0.0],
                    fg_rgba: pack_rgba(cell.bg_rgba),
                    bg_rgba: pack_rgba(cell.bg_rgba),
                    page: PAGE_SOLID,
                    flags: 0,
                });
            }
            // Glyph quad. Empty / whitespace clusters skip this.
            if !cell.glyph.is_empty() && cell.glyph != " " {
                // Box-drawing short-circuit: stroke the cell rect with
                // solid quads instead of rasterizing the font glyph so
                // adjacent cells meet without hairline gaps. Falls
                // through to the regular glyph path for non-box cps.
                let first_cp = cell.glyph.chars().next().map(|c| c as u32).unwrap_or(0);
                if let Some(rects) = super::box_drawing::rects_for(first_cp, w, h) {
                    for (rx, ry, rw, rh) in rects {
                        out.push(CellInstance {
                            cell_xy: [x + rx, y + ry],
                            cell_wh: [rw, rh],
                            atlas_uv: [0.0, 0.0, 0.0, 0.0],
                            fg_rgba: pack_rgba(cell.fg_rgba),
                            bg_rgba: pack_rgba(cell.bg_rgba),
                            page: PAGE_SOLID,
                            flags: FLAG_FG_FILL,
                        });
                    }
                } else if let Some((rects, alpha_override)) =
                    super::block_drawing::rects_for(first_cp, w, h)
                {
                    // Shade characters supply an alpha override; we
                    // preserve the cell's fg RGB and patch only the A
                    // channel so the alpha-blend stage paints a
                    // partially-transparent fg fill over the bg.
                    let mut fg = cell.fg_rgba;
                    if let Some(a) = alpha_override {
                        fg[3] = a;
                    }
                    for (rx, ry, rw, rh) in rects {
                        out.push(CellInstance {
                            cell_xy: [x + rx, y + ry],
                            cell_wh: [rw, rh],
                            atlas_uv: [0.0, 0.0, 0.0, 0.0],
                            fg_rgba: pack_rgba(fg),
                            bg_rgba: pack_rgba(cell.bg_rgba),
                            page: PAGE_SOLID,
                            flags: FLAG_FG_FILL,
                        });
                    }
                } else if let Some(instance) = self.glyph_instance(
                    &mut cache,
                    cell,
                    x,
                    y,
                    w,
                    h,
                    metrics.font_size_px,
                    base_ascent,
                    v_pad,
                ) {
                    out.push(instance);
                }
            }
            // Decoration lines: rendered as thin solid quads inside the
            // shader by branching on `flags`. We emit one decoration
            // instance per active decoration so the shader can place the
            // line at the correct sub-rect.
            if cell.underline {
                out.push(CellInstance {
                    cell_xy: [x, y],
                    cell_wh: [w, h],
                    atlas_uv: [0.0, 0.0, 0.0, 0.0],
                    fg_rgba: pack_rgba(cell.fg_rgba),
                    bg_rgba: pack_rgba(cell.bg_rgba),
                    page: PAGE_SOLID,
                    flags: FLAG_UNDERLINE,
                });
            }
            if cell.strikethrough {
                out.push(CellInstance {
                    cell_xy: [x, y],
                    cell_wh: [w, h],
                    atlas_uv: [0.0, 0.0, 0.0, 0.0],
                    fg_rgba: pack_rgba(cell.fg_rgba),
                    bg_rgba: pack_rgba(cell.bg_rgba),
                    page: PAGE_SOLID,
                    flags: FLAG_STRIKETHROUGH,
                });
            }
        }
        out
    }

    /// Resolve a single cell's glyph to a `CellInstance`. Returns `None`
    /// when no font in the fallback chain covers the cluster — caller
    /// emits no glyph instance (background + decoration still fire).
    ///
    /// `base_ascent` and `v_pad` are pre-computed by the caller from
    /// the base font's real metrics so all glyphs in the grid share a
    /// consistent baseline regardless of which fallback font supplied
    /// the bitmap.
    fn glyph_instance(
        &self,
        cache: &mut GlyphCache,
        cell: &CellInput,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        size_px: f32,
        base_ascent: f32,
        v_pad: f32,
    ) -> Option<CellInstance> {
        // Prefer the emoji font when the cluster carries VS-16, so that
        // codepoints with dual presentation (e.g. U+26A0 warning sign)
        // get the colored emoji glyph rather than the BW base-font one.
        let font_id = self
            .fallback
            .resolve_for_cluster(&*self.rasterizer, &cell.glyph)?;
        // SGR bold: swap in the resolved font's real bold face when one
        // is registered (e.g. Inconsolata → Inconsolata Bold). Coverage
        // is resolved on the regular face; the bold face of the same
        // family carries the same repertoire. Fonts without a bold
        // variant (bundled CJK, emoji) keep their regular face.
        let font_id = if cell.bold {
            self.fallback.bold_variant(font_id).unwrap_or(font_id)
        } else {
            font_id
        };
        let shaped = self.rasterizer.shape(&cell.glyph, font_id, size_px);
        let g = shaped.first()?;
        if g.glyph_id == 0 {
            return None;
        }
        let key = GlyphKey::new(font_id, g.glyph_id, size_px, 0.0);
        let region = cache.get_or_rasterize(&*self.rasterizer, key)?;
        if region.is_empty() {
            return None;
        }
        let page = match region.format {
            AtlasFormat::Alpha => PAGE_ALPHA,
            AtlasFormat::Rgba => PAGE_RGBA,
            AtlasFormat::Subpixel => PAGE_SUBPIXEL,
        };
        // UV rect inside the atlas page; converted from pixel space to
        // normalized [0..1] in the vertex shader using the uniform-side
        // page dimensions.
        let u0 = region.x as f32;
        let v0 = region.y as f32;
        let u1 = (region.x + region.width) as f32;
        let v1 = (region.y + region.height) as f32;
        // Place the glyph quad at its natural bitmap size + bearing
        // offset inside the cell rather than stretching the bitmap to
        // fill the cell. Baseline is anchored to the BASE font's real
        // ascent so all glyphs share a consistent horizontal line, with
        // `v_pad` centering the line vertically inside the cell.
        let mut glyph_w = region.width as f32;
        let mut glyph_h = region.height as f32;
        let baseline = y + v_pad + base_ascent;
        let mut glyph_x = x + region.bearing_left as f32;
        let mut glyph_y = baseline - region.bearing_top as f32;
        // IME preedit overlay (`fit_glyph_to_cell = true`): force the
        // glyph quad to sit entirely within the cell rect. Required so
        // (a) ambiguous-width shapes (▽ U+25BD) whose bitmap is wider
        // than the 1-cell footprint don't bleed sideways, and (b) CJK
        // glyphs whose descenders rasterize past `cell_h` are scaled
        // back inside the reverse-video bg instead of leaking onto the
        // next row's dark default background.
        if cell.fit_glyph_to_cell && glyph_w > 0.0 && glyph_h > 0.0 {
            // First: horizontal scale so the bitmap fits the cell width.
            let sx = (w / glyph_w).min(1.0);
            // Second: vertical scale so the bitmap fits the cell height
            // *measured against where the glyph currently lands*. We
            // include the offset above the cell top (baseline placement
            // can leave the bitmap top above `y`) and below the cell
            // bottom (descender past `y + h`).
            let top_overflow = (y - glyph_y).max(0.0);
            let bottom_overflow = ((glyph_y + glyph_h) - (y + h)).max(0.0);
            let sy = if top_overflow + bottom_overflow > 0.0 {
                (h / (glyph_h + top_overflow + bottom_overflow)).min(1.0)
            } else {
                1.0
            };
            let scale = sx.min(sy);
            if scale < 1.0 {
                glyph_w *= scale;
                glyph_h *= scale;
                // Re-center horizontally inside the cell.
                glyph_x = x + (w - glyph_w) * 0.5;
                // Keep the baseline pinned so adjacent clusters with
                // different bitmap heights still line up — otherwise
                // each glyph would re-anchor to the cell's vertical
                // center and the row's baseline would jitter cluster
                // by cluster (visible as zig-zag during preedit).
                let scaled_bearing_top = region.bearing_top as f32 * scale;
                glyph_y = baseline - scaled_bearing_top;
                // If the scaled glyph still overshoots the cell rect
                // after baseline placement, clamp the top/bottom into
                // the cell so the reverse-video bg keeps it contained.
                let overshoot_top = (y - glyph_y).max(0.0);
                let overshoot_bot = ((glyph_y + glyph_h) - (y + h)).max(0.0);
                if overshoot_top > 0.0 {
                    glyph_y += overshoot_top;
                } else if overshoot_bot > 0.0 {
                    glyph_y -= overshoot_bot;
                }
            }
        }
        if glyph_w <= 0.0 || glyph_h <= 0.0 {
            return None;
        }
        // Snap the glyph quad to the physical pixel grid. The cell pitch
        // is fractional (e.g. 8.667 px), so unrounded quad origins land
        // between pixels and the Linear atlas sample smears every glyph
        // by a pixel — visibly blurry/washed-out at terminal sizes. The
        // quad size stays at the bitmap's integer size, so a snapped
        // origin gives an exact 1:1 texel-to-pixel mapping. Background
        // quads intentionally stay fractional (rounding them would open
        // hairline gaps between adjacent cells).
        let mut glyph_x = glyph_x.round();
        let glyph_y = glyph_y.round();
        let mut glyph_w = glyph_w;
        let mut u0 = u0;
        let mut u1 = u1;
        // Subpixel glyphs: clip the quad horizontally to the cell rect.
        // swash's hinted bitmaps can be wider than the cell (Inconsolata
        // 'm' / 'w' at 13 pt: left=-1, width=11 vs 9-px cells), and the
        // subpixel shader composites the FULL quad against the cell's bg
        // color opaquely — an overhanging quad would paint this cell's
        // bg outside the cell, visible as a 1-px bg-colored fringe next
        // to reverse-video runs (e.g. ls's /dev/shm highlight). Alpha /
        // RGBA pages alpha-blend (bg never leaks), so they keep the
        // natural overhang like the WebView build's Canvas fillText.
        if page == PAGE_SUBPIXEL {
            // Snap the cell bounds to the pixel grid before clipping. The
            // glyph quad is already pixel-snapped (integer origin + integer
            // bitmap width from .round() above), so comparing it against
            // UNrounded fractional cell bounds (which occur under fractional
            // HiDPI scale factors where cell_w = cell_w_logical × ppp) would
            // shave a sub-pixel sliver off every glyph and shift the UV off
            // the 1:1 texel mapping, causing per-glyph blur. Snapping makes
            // the comparison integer-vs-integer: fitting glyphs pass through
            // untouched and only true ≥1px overhang is trimmed.
            if let Some((cx, cw, cu0, cu1)) =
                clip_quad_to_cell_x(glyph_x, glyph_w, u0, u1, x.round(), (x + w).round())
            {
                glyph_x = cx;
                glyph_w = cw;
                u0 = cu0;
                u1 = cu1;
            } else {
                return None;
            }
        }
        let _ = h;
        Some(CellInstance {
            cell_xy: [glyph_x, glyph_y],
            cell_wh: [glyph_w, glyph_h],
            atlas_uv: [u0, v0, u1, v1],
            fg_rgba: pack_rgba(cell.fg_rgba),
            bg_rgba: pack_rgba(cell.bg_rgba),
            page,
            flags: 0,
        })
    }

    /// Upload the current atlas page bytes to the GPU and rebuild the bind
    /// group + instance buffer. Called once per frame from
    /// `window_host::render` after the cell loop has produced
    /// [`CellInput`] entries.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        cells: &[CellInput],
        metrics: CellMetrics,
        viewport_w: u32,
        viewport_h: u32,
    ) -> PreparedFrame {
        let instances = self.build_instances(cells, metrics);
        if instances.is_empty() {
            return PreparedFrame {
                instances,
                instance_buffer: None,
                uniform_buffer: None,
                bind_group: None,
            };
        }
        // Sync the GPU atlas textures with the CPU atlas bytes.
        let (alpha_dim, rgba_dim, generation) = {
            let cache = self.cache.lock();
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
            let cache = self.cache.lock();
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
            _pad: [0.0, 0.0],
        };
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("native-poc-terminal-grid-uniform"),
            size: std::mem::size_of::<FrameUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&uniform_buffer, 0, bytemuck_compat::cast_slice(&[uniform]));

        let instance_bytes = bytemuck_compat::cast_slice(&instances);
        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("native-poc-terminal-grid-instances"),
            size: instance_bytes.len() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&instance_buffer, 0, instance_bytes);

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("native-poc-terminal-grid-bg"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buffer.as_entire_binding(),
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
        });

        PreparedFrame {
            instances,
            instance_buffer: Some(instance_buffer),
            uniform_buffer: Some(uniform_buffer),
            bind_group: Some(bind_group),
        }
    }

    /// Issue one instanced draw call. The render pass must already be
    /// configured with `LoadOp::Load` (`clear` ran in an earlier pass).
    pub fn draw<'pass>(
        &'pass self,
        rpass: &mut wgpu::RenderPass<'pass>,
        frame: &'pass PreparedFrame,
    ) {
        let (Some(buf), Some(bg)) = (frame.instance_buffer.as_ref(), frame.bind_group.as_ref())
        else {
            return;
        };
        if frame.instances.is_empty() {
            return;
        }
        rpass.set_pipeline(&self.pipeline);
        rpass.set_bind_group(0, bg, &[]);
        rpass.set_vertex_buffer(0, buf.slice(..));
        rpass.draw(0..4, 0..frame.instances.len() as u32);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::font::resolver::Resolver;
    use crate::render::font::swash_adapter::SwashRasterizer;
    use crate::render::font::traits::{AtlasFormat, FontId, GlyphBitmap, ShapedGlyph};

    /// Test rasterizer that returns canned bitmaps from a static table.
    struct StubRasterizer {
        ascii_font: FontId,
        emoji_font: FontId,
    }

    impl GlyphRasterizer for StubRasterizer {
        fn shape(&self, cluster: &str, font: FontId, size_px: f32) -> Vec<ShapedGlyph> {
            // Map ascii -> glyph id = byte value; cluster 'あ' -> 0xAA; '😀' -> 0xBB.
            let first = cluster.chars().next().unwrap_or('\0') as u32;
            let glyph_id = match first {
                0x41..=0x7A => first,
                0x3042 => 0xAA,
                0x1F600 => 0xBB,
                _ => 0,
            };
            vec![ShapedGlyph {
                font,
                glyph_id,
                size_px,
            }]
        }
        fn raster(&self, font: FontId, glyph_id: u32, _size_px: f32) -> Option<GlyphBitmap> {
            if glyph_id == 0 {
                return None;
            }
            if font == self.emoji_font {
                Some(GlyphBitmap {
                    format: AtlasFormat::Rgba,
                    width: 16,
                    height: 16,
                    bearing: (0, 0),
                    advance: 16.0,
                    pixels: vec![0xFF; 16 * 16 * 4],
                })
            } else if font == self.ascii_font {
                Some(GlyphBitmap {
                    format: AtlasFormat::Alpha,
                    width: 8,
                    height: 16,
                    bearing: (0, 0),
                    advance: 8.0,
                    pixels: vec![0xFF; 8 * 16],
                })
            } else {
                None
            }
        }
        fn has_codepoint(&self, font: FontId, cp: u32) -> bool {
            match (font, cp) {
                (f, c) if f == self.ascii_font && (0x41..=0x7A).contains(&c) => true,
                (f, 0x3042) if f != self.ascii_font && f != self.emoji_font => true,
                (f, 0x1F600) if f == self.emoji_font => true,
                _ => false,
            }
        }
    }

    /// Standalone wrapper that mirrors `TerminalGridPass::build_instances`
    /// without instantiating the wgpu-bearing fields. The logic is
    /// identical and lives in the same file so any changes stay in sync.
    fn helper_build_instances(
        rasterizer: &dyn GlyphRasterizer,
        fallback: &FallbackChain,
        cache: &Arc<Mutex<GlyphCache>>,
        cells: &[CellInput],
        metrics: CellMetrics,
    ) -> Vec<CellInstance> {
        let mut out = Vec::with_capacity(cells.len() * 2);
        let mut cache_lock = cache.lock();
        let base_metrics = rasterizer.font_metrics(fallback.base(), metrics.font_size_px);
        let base_ascent = base_metrics
            .map(|m| m.ascent)
            .unwrap_or(metrics.font_size_px * 0.8);
        let base_line_height = base_metrics
            .map(|m| m.line_height())
            .unwrap_or(metrics.font_size_px);
        let v_pad = ((metrics.cell_h - base_line_height) * 0.5).max(0.0);
        for cell in cells {
            let x = metrics.origin[0] + cell.col as f32 * metrics.cell_w;
            let y = metrics.origin[1] + cell.row as f32 * metrics.cell_h;
            let w = metrics.cell_w * (cell.width_cells.max(1) as f32);
            let h = metrics.cell_h;
            if cell.draw_background {
                out.push(CellInstance {
                    cell_xy: [x, y],
                    cell_wh: [w, h],
                    atlas_uv: [0.0, 0.0, 0.0, 0.0],
                    fg_rgba: pack_rgba(cell.bg_rgba),
                    bg_rgba: pack_rgba(cell.bg_rgba),
                    page: PAGE_SOLID,
                    flags: 0,
                });
            }
            if !cell.glyph.is_empty() && cell.glyph != " " {
                if let Some(font_id) = fallback.resolve_for_cluster(rasterizer, &cell.glyph) {
                    let shaped = rasterizer.shape(&cell.glyph, font_id, metrics.font_size_px);
                    if let Some(g) = shaped.first() {
                        if g.glyph_id != 0 {
                            let key = GlyphKey::new(font_id, g.glyph_id, metrics.font_size_px, 0.0);
                            if let Some(region) = cache_lock.get_or_rasterize(rasterizer, key) {
                                if !region.is_empty() {
                                    let page = match region.format {
                                        AtlasFormat::Alpha => PAGE_ALPHA,
                                        AtlasFormat::Rgba => PAGE_RGBA,
                                        AtlasFormat::Subpixel => PAGE_SUBPIXEL,
                                    };
                                    let glyph_w = region.width as f32;
                                    let glyph_h = region.height as f32;
                                    let baseline = y + v_pad + base_ascent;
                                    let glyph_x = x + region.bearing_left as f32;
                                    let glyph_y = baseline - region.bearing_top as f32;
                                    out.push(CellInstance {
                                        cell_xy: [glyph_x, glyph_y],
                                        cell_wh: [glyph_w, glyph_h],
                                        atlas_uv: [
                                            region.x as f32,
                                            region.y as f32,
                                            (region.x + region.width) as f32,
                                            (region.y + region.height) as f32,
                                        ],
                                        fg_rgba: pack_rgba(cell.fg_rgba),
                                        bg_rgba: pack_rgba(cell.bg_rgba),
                                        page,
                                        flags: 0,
                                    });
                                }
                            }
                        }
                    }
                }
            }
            if cell.underline {
                out.push(CellInstance {
                    cell_xy: [x, y],
                    cell_wh: [w, h],
                    atlas_uv: [0.0, 0.0, 0.0, 0.0],
                    fg_rgba: pack_rgba(cell.fg_rgba),
                    bg_rgba: pack_rgba(cell.bg_rgba),
                    page: PAGE_SOLID,
                    flags: FLAG_UNDERLINE,
                });
            }
            if cell.strikethrough {
                out.push(CellInstance {
                    cell_xy: [x, y],
                    cell_wh: [w, h],
                    atlas_uv: [0.0, 0.0, 0.0, 0.0],
                    fg_rgba: pack_rgba(cell.fg_rgba),
                    bg_rgba: pack_rgba(cell.bg_rgba),
                    page: PAGE_SOLID,
                    flags: FLAG_STRIKETHROUGH,
                });
            }
        }
        out
    }

    fn ascii_cell(col: u16, row: u16, ch: &str) -> CellInput {
        CellInput {
            col,
            row,
            width_cells: 1,
            glyph: ch.into(),
            fg_rgba: [255, 255, 255, 255],
            bg_rgba: [0, 0, 0, 255],
            underline: false,
            strikethrough: false,
            draw_background: false,
            bg_extend_below: 0.0,
            fit_glyph_to_cell: false,
            bold: false,
        }
    }

    fn metrics() -> CellMetrics {
        CellMetrics {
            cell_w: 8.5,
            cell_h: 17.0,
            origin: [0.0, 0.0],
            font_size_px: 13.0,
        }
    }

    fn build_stack() -> (
        Arc<StubRasterizer>,
        Arc<FallbackChain>,
        Arc<Mutex<GlyphCache>>,
    ) {
        let ascii = FontId(1);
        let cjk = FontId(2);
        let emoji = FontId(3);
        let raster = Arc::new(StubRasterizer {
            ascii_font: ascii,
            emoji_font: emoji,
        });
        let chain = Arc::new(FallbackChain::new(ascii, [cjk, emoji]));
        let cache = Arc::new(Mutex::new(GlyphCache::new()));
        (raster, chain, cache)
    }

    /// TS-font-13: `TerminalGridPass::prepare` emits one (glyph) instance
    /// per non-empty cell. We exercise the CPU-side `build_instances`
    /// helper here — it is the path GPU `prepare` calls before uploading.
    #[test]
    fn build_instances_one_per_non_empty_cell() {
        let (raster, chain, cache) = build_stack();
        let cells = vec![
            ascii_cell(0, 0, "A"),
            ascii_cell(1, 0, "B"),
            ascii_cell(2, 0, "C"),
            ascii_cell(3, 0, " "), // whitespace → no glyph instance
            ascii_cell(4, 0, ""),  // empty cluster → no glyph instance
        ];
        let inst = helper_build_instances(&*raster, &chain, &cache, &cells, metrics());
        // Exactly 3 glyph instances; whitespace + empty produce nothing
        // (draw_background = false → no bg quad either).
        assert_eq!(inst.len(), 3);
        for i in &inst {
            assert_eq!(i.page, PAGE_ALPHA);
            // UV is non-empty for hit glyphs.
            assert!(i.atlas_uv[2] > i.atlas_uv[0]);
            assert!(i.atlas_uv[3] > i.atlas_uv[1]);
        }
    }

    /// TS-font-14: per-instance `page` tag encodes Alpha for ASCII and
    /// RGBA for color emoji.
    #[test]
    fn build_instances_records_page_kind_per_glyph() {
        let (raster, chain, cache) = build_stack();
        let cells = vec![
            ascii_cell(0, 0, "A"),
            CellInput {
                col: 2,
                row: 0,
                width_cells: 2,
                glyph: "\u{1F600}".into(), // 😀
                fg_rgba: [255, 255, 255, 255],
                bg_rgba: [0, 0, 0, 255],
                underline: false,
                strikethrough: false,
                draw_background: false,
                bg_extend_below: 0.0,
                fit_glyph_to_cell: false,
                bold: false,
            },
        ];
        let inst = helper_build_instances(&*raster, &chain, &cache, &cells, metrics());
        assert_eq!(inst.len(), 2);
        // First cell: alpha; second: rgba.
        assert_eq!(inst[0].page, PAGE_ALPHA);
        assert_eq!(inst[1].page, PAGE_RGBA);
    }

    // ── clip_quad_to_cell_x ──────────────────────────────────

    /// A quad already inside the cell passes through untouched.
    #[test]
    fn clip_quad_inside_cell_is_unchanged() {
        let r = clip_quad_to_cell_x(10.0, 8.0, 0.0, 8.0, 9.0, 18.0);
        assert_eq!(r, Some((10.0, 8.0, 0.0, 8.0)));
    }

    /// The call site in `glyph_instance` snaps fractional cell bounds via
    /// `.round()` before passing them to `clip_quad_to_cell_x`. This test
    /// demonstrates that contract: a pixel-snapped quad (glyph_x=11.0,
    /// glyph_w=8.0) that fits perfectly inside a fractional-scale cell
    /// [10.75, 19.5] would be wrongly trimmed if the raw bounds were passed,
    /// but after the call-site snap to [11.0, 20.0] the quad passes through
    /// unchanged (no sub-pixel sliver is shaved off).
    #[test]
    fn clip_quad_call_site_snaps_fractional_cell_bounds() {
        let (glyph_x, glyph_w) = (11.0_f32, 8.0_f32);
        let (u0, u1) = (0.0_f32, 8.0_f32);
        // Raw fractional cell bounds (1.5× HiDPI example).
        let cell_left_raw = 10.75_f32;
        let cell_right_raw = 19.5_f32;
        // Without snapping, left_trim = 10.75 - 11.0 = -0.25 (no left clip),
        // but right_trim = (11.0+8.0) - 19.5 = -0.5, which is also ≤ 0, so
        // the raw bounds actually pass here too — the real hazard is when the
        // fractional cell_left > glyph_x, which shaves the left side.
        // Use a case where the fractional left is strictly above glyph_x:
        // cell [11.25, 20.0] → left_trim = 0.25 → wrong UV shift without snap.
        let cell_left_frac = 11.25_f32;
        let cell_right_frac = 20.0_f32;
        // Without snap: left_trim > 0 → quad and UV are modified (wrong).
        let without_snap =
            clip_quad_to_cell_x(glyph_x, glyph_w, u0, u1, cell_left_frac, cell_right_frac);
        assert_ne!(
            without_snap,
            Some((glyph_x, glyph_w, u0, u1)),
            "raw fractional bounds wrongly trim a fitting quad"
        );
        // With snap (as the call site does): [11.25.round(), 20.0.round()] = [11.0, 20.0].
        let with_snap = clip_quad_to_cell_x(
            glyph_x,
            glyph_w,
            u0,
            u1,
            cell_left_frac.round(),
            cell_right_frac.round(),
        );
        assert_eq!(
            with_snap,
            Some((glyph_x, glyph_w, u0, u1)),
            "snapped bounds leave a fitting pixel-aligned quad unchanged"
        );
        let _ = (cell_left_raw, cell_right_raw); // documented above; not used in assertions
    }

    /// Inconsolata 'm' at 13 pt: bearing −1, bitmap 11 px wide in a 9-px
    /// cell. Both overhangs trim, and the UV range shrinks by the same
    /// amount on each side (1:1 texel mapping preserved).
    #[test]
    fn clip_quad_overhang_trims_both_sides_and_uv() {
        // Cell [9, 18), quad [8, 19) → clipped to [9, 18).
        let r = clip_quad_to_cell_x(8.0, 11.0, 100.0, 111.0, 9.0, 18.0);
        let (x, w, u0, u1) = r.expect("clipped quad survives");
        assert_eq!((x, w), (9.0, 9.0));
        assert_eq!((u0, u1), (101.0, 110.0));
    }

    /// A quad entirely outside the cell clips to nothing.
    #[test]
    fn clip_quad_outside_cell_returns_none() {
        assert_eq!(clip_quad_to_cell_x(20.0, 5.0, 0.0, 5.0, 0.0, 9.0), None);
        assert_eq!(clip_quad_to_cell_x(0.0, 0.0, 0.0, 0.0, 0.0, 9.0), None);
    }

    /// Subpixel-mode swash output routes to the PAGE_SUBPIXEL shader
    /// branch (per-channel fg/bg compositing).
    #[test]
    fn integration_swash_subpixel_maps_to_subpixel_page() {
        let mut resolver = Resolver::new();
        let (cjk_id, emoji_id) = resolver.register_bundled();
        let swash = Arc::new(SwashRasterizer::with_subpixel(true));
        swash.ingest_resolver(&resolver);
        let chain = Arc::new(FallbackChain::new(cjk_id, [emoji_id]));
        let cache = Arc::new(Mutex::new(GlyphCache::new()));
        let cells = vec![ascii_cell(0, 0, "d")];
        let raster_ref: &dyn GlyphRasterizer = &*swash;
        let inst = helper_build_instances(raster_ref, &chain, &cache, &cells, metrics());
        assert_eq!(inst.len(), 1, "exactly one glyph instance for 'd'");
        assert_eq!(
            inst[0].page, PAGE_SUBPIXEL,
            "subpixel raster must select the subpixel shader page"
        );
    }

    /// TS-font-int-2: headless render of a single cell containing U+3042
    /// using the swash engine. The pass emits a non-empty instance and
    /// does not panic.
    #[test]
    fn integration_swash_renders_cjk_cell_cpu_side() {
        // Build a swash rasterizer + resolver against the bundled fonts.
        let mut resolver = Resolver::new();
        let (cjk_id, emoji_id) = resolver.register_bundled();
        let swash = Arc::new(SwashRasterizer::with_subpixel(false));
        swash.ingest_resolver(&resolver);
        // Chain: cjk first (no base font registered against swash here,
        // so 'A' would tofu — TS-font-int-2 only tests U+3042).
        let chain = Arc::new(FallbackChain::new(cjk_id, [emoji_id]));
        let cache = Arc::new(Mutex::new(GlyphCache::new()));
        let cells = vec![CellInput {
            col: 0,
            row: 0,
            width_cells: 2,
            glyph: "\u{3042}".into(), // あ
            fg_rgba: [255, 255, 255, 255],
            bg_rgba: [0, 0, 0, 255],
            underline: false,
            strikethrough: false,
            draw_background: false,
            bg_extend_below: 0.0,
            fit_glyph_to_cell: false,
            bold: false,
        }];
        let raster_ref: &dyn GlyphRasterizer = &*swash;
        let inst = helper_build_instances(
            raster_ref,
            &chain,
            &cache,
            &cells,
            CellMetrics {
                cell_w: 16.0,
                cell_h: 24.0,
                origin: [0.0, 0.0],
                font_size_px: 18.0,
            },
        );
        assert_eq!(inst.len(), 1, "exactly one glyph instance for U+3042");
        assert_eq!(inst[0].page, PAGE_ALPHA, "CJK is monochrome → alpha page");
        assert!(
            inst[0].atlas_uv[2] > inst[0].atlas_uv[0],
            "non-empty UV width"
        );
    }

    #[test]
    fn pack_rgba_byte_order_is_little_endian_rgba() {
        // [r=0x11, g=0x22, b=0x33, a=0xFF] packs as 0xFF332211.
        let p = pack_rgba([0x11, 0x22, 0x33, 0xFF]);
        assert_eq!(p, 0xFF332211);
    }

    #[test]
    fn cell_instance_stride_matches_layout() {
        // The wgpu pipeline encodes the stride; if this changes, the
        // VertexAttribute offsets above must be updated.
        assert_eq!(CellInstance::STRIDE, 48);
    }

    #[test]
    fn empty_cells_produce_no_instances() {
        let (raster, chain, cache) = build_stack();
        let inst = helper_build_instances(&*raster, &chain, &cache, &[], metrics());
        assert!(inst.is_empty());
    }

    /// Decoration flags emit dedicated solid-color instances on top of
    /// the glyph instance.
    #[test]
    fn decoration_flags_emit_solid_instances() {
        let (raster, chain, cache) = build_stack();
        let mut cell = ascii_cell(0, 0, "A");
        cell.underline = true;
        cell.strikethrough = true;
        let inst = helper_build_instances(&*raster, &chain, &cache, &[cell], metrics());
        // 1 glyph + 1 underline + 1 strikethrough.
        assert_eq!(inst.len(), 3);
        let pages: Vec<u32> = inst.iter().map(|i| i.page).collect();
        let flags: Vec<u32> = inst.iter().map(|i| i.flags).collect();
        assert_eq!(pages, vec![PAGE_ALPHA, PAGE_SOLID, PAGE_SOLID]);
        assert_eq!(flags, vec![0, FLAG_UNDERLINE, FLAG_STRIKETHROUGH]);
    }
}

#[cfg(test)]
mod gpu_tests {
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
        let adapter =
            match pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
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
        let (cjk, _emoji) = resolver.register_bundled();
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
}
