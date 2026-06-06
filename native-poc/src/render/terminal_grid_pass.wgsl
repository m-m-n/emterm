// WGSL shader for the terminal grid render pass.
//
// One instanced draw call produces every cell in the active grid: the
// vertex shader synthesises a unit quad and scales it to the instance's
// `cell_xy`/`cell_wh` rect; the fragment shader branches on
// `atlas_page_kind` to sample the right atlas page (Alpha R8 modulated by
// fg, or Rgba sampled directly), or emits a solid color for background +
// decoration instances.
//
// Page kinds:
//   0 = Alpha (R8, fg modulation)
//   1 = Rgba (RGBA8, sampled as-is)
//   2 = Solid (no atlas read; used for background + decoration lines)
//   3 = Subpixel (RGBA8 coverage mask on the RGBA page; per-channel
//       fg/bg blend — LCD anti-aliasing)
//
// Decoration flags packed into `flags`:
//   bit 0 = underline (1-px line at the cell bottom)
//   bit 1 = strikethrough (1-px line at the cell midpoint)

struct FrameUniform {
    viewport: vec2<f32>,
    alpha_atlas: vec2<f32>,
    rgba_atlas: vec2<f32>,
    _pad: vec2<f32>,
};

@group(0) @binding(0) var<uniform> u: FrameUniform;
@group(0) @binding(1) var t_alpha: texture_2d<f32>;
@group(0) @binding(2) var t_rgba: texture_2d<f32>;
@group(0) @binding(3) var s_atlas: sampler;

struct VsIn {
    @builtin(vertex_index) vid: u32,
    @location(0) cell_xy: vec2<f32>,
    @location(1) cell_wh: vec2<f32>,
    @location(2) atlas_uv: vec4<f32>,
    @location(3) fg_rgba: u32,
    @location(4) bg_rgba: u32,
    @location(5) page: u32,
    @location(6) flags: u32,
};

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) fg: vec4<f32>,
    @location(2) bg: vec4<f32>,
    @location(3) @interpolate(flat) page: u32,
    @location(4) @interpolate(flat) flags: u32,
    @location(5) cell_local: vec2<f32>,
};

@vertex
fn vs_main(in: VsIn) -> VsOut {
    // Corners: 0=(0,0), 1=(1,0), 2=(0,1), 3=(1,1).
    var corners = array<vec2<f32>, 4>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 1.0),
    );
    let c = corners[in.vid];

    // Pixel-space position of this corner inside the swapchain.
    let px = in.cell_xy.x + c.x * in.cell_wh.x;
    let py = in.cell_xy.y + c.y * in.cell_wh.y;

    // Map pixel space → clip space [-1, +1] with Y flipped (wgpu fragment
    // coordinate origin is top-left but clip space is Y-up).
    let cx = (px / u.viewport.x) * 2.0 - 1.0;
    let cy = 1.0 - (py / u.viewport.y) * 2.0;

    // Atlas UV: interpolate between the (u0,v0) and (u1,v1) corners of
    // the instance's atlas region, normalising by the active atlas page
    // dimensions in the fragment shader (the atlas page depends on the
    // page tag, so we cannot pick a divisor here).
    let u0 = in.atlas_uv.x;
    let v0 = in.atlas_uv.y;
    let u1 = in.atlas_uv.z;
    let v1 = in.atlas_uv.w;
    let uv_px = vec2<f32>(
        mix(u0, u1, c.x),
        mix(v0, v1, c.y),
    );

    var out: VsOut;
    out.clip = vec4<f32>(cx, cy, 0.0, 1.0);
    out.uv = uv_px;
    out.fg = unpack4x8unorm(in.fg_rgba);
    out.bg = unpack4x8unorm(in.bg_rgba);
    out.page = in.page;
    out.flags = in.flags;
    out.cell_local = c; // 0..1 within the cell rect
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // Solid page (background or decoration line or fg-fill stroke).
    if (in.page == 2u) {
        // Decoration lines: clip the visible band based on flags.
        if ((in.flags & 1u) != 0u) {
            // Underline: a thin band near the bottom of the cell.
            //   visible when cell_local.y in [0.92, 0.98]
            if (in.cell_local.y < 0.92 || in.cell_local.y > 0.98) {
                discard;
            }
            return in.fg;
        }
        if ((in.flags & 2u) != 0u) {
            // Strikethrough: thin band at ~55% of cell height.
            if (in.cell_local.y < 0.52 || in.cell_local.y > 0.58) {
                discard;
            }
            return in.fg;
        }
        if ((in.flags & 4u) != 0u) {
            // Foreground-color fill (box drawing strokes, block-element
            // rects, shade alpha-blends). The instance carries the
            // already-scaled rect in cell_xy/cell_wh; the shader just
            // paints fg straight through the alpha blend stage.
            return in.fg;
        }
        // Plain background fill.
        return in.bg;
    }

    if (in.page == 0u) {
        // Alpha glyph: sample R from the Alpha atlas, modulate fg color.
        let uv = in.uv / u.alpha_atlas;
        let a = textureSample(t_alpha, s_atlas, uv).r;
        // Pre-multiplied alpha so the SrcAlpha / OneMinusSrcAlpha blend
        // composites correctly over the cleared / lower passes.
        return vec4<f32>(in.fg.rgb * a, in.fg.a * a);
    }

    if (in.page == 3u) {
        // Subpixel glyph (LCD anti-aliasing): the RGBA page holds a
        // per-channel coverage mask (R/G/B rasterized at ∓1/3-px
        // horizontal offsets). Per-channel coverage is folded into a
        // single coverage alpha `a` (the strongest channel) so that
        // mask==0 texels stay fully transparent — overhanging quads
        // (descenders, CJK glyphs taller than the cell) no longer paint
        // this cell's bg over neighboring rows. Where the quad sits on
        // this cell's own bg quad (dst == bg) the SrcAlpha /
        // OneMinusSrcAlpha blend resolves to exactly
        //   fg*mask + bg*(1-mask)
        // — identical to the previous opaque per-channel composite.
        let uv = in.uv / u.rgba_atlas;
        let mask = textureSample(t_rgba, s_atlas, uv).rgb;
        // Coverage alpha: the strongest channel.
        let a = max(mask.r, max(mask.g, mask.b));
        if (a <= 0.0) {
            discard;
        }
        let rgb = (in.fg.rgb * mask + in.bg.rgb * (vec3<f32>(a) - mask)) / a;
        return vec4<f32>(rgb, a);
    }

    // RGBA glyph: sample the color atlas directly. The atlas page holds
    // sRGB-encoded premultiplied bytes in Rgba8Unorm (non-sRGB) storage,
    // so sampling returns them un-decoded and the blend stage composites
    // in gamma space — matching the WebView build's Canvas 2D pipeline
    // and the non-sRGB surface format.
    let uv = in.uv / u.rgba_atlas;
    let c = textureSample(t_rgba, s_atlas, uv);
    return c;
}
