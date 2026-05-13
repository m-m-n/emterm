//! wgpu textured-quad pipeline for [`super::ImageLayer`].
//!
//! Renders a single source-over blended quad per placement. Designed to be
//! invoked *after* the egui pass on the same swapchain texture using
//! `LoadOp::Load` so the egui-rendered terminal cells underneath are
//! preserved.
//!
//! The shader is a minimal vertex-pulling pipeline: each draw call pulls
//! four vertices from constants in the vertex shader, builds clip-space
//! coordinates from a `Viewport` + `Placement` uniform, and samples the
//! texture in the fragment shader.

use super::ImageLayer;

/// Inline WGSL shader. Vertex shader synthesises a unit quad from the
/// vertex index (0..3) and a single `PlacementUniform`; fragment shader
/// samples the bound texture with `clamp-to-edge` (so we never read
/// outside the source image even when sub-pixel rounding pushes us off
/// by one).
const SHADER_SRC: &str = r#"
struct PlacementUniform {
    // Pixel rect (x, y, w, h) of the placement in the swapchain.
    rect: vec4<f32>,
    // Swapchain size in pixels (w, h).
    viewport: vec2<f32>,
    _pad: vec2<f32>,
};

@group(0) @binding(0) var<uniform> u: PlacementUniform;
@group(0) @binding(1) var t_image: texture_2d<f32>;
@group(0) @binding(2) var s_image: sampler;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VsOut {
    // Quad indices 0..3 → corners (0,0) (1,0) (0,1) (1,1).
    var corners = array<vec2<f32>, 4>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 1.0),
    );
    let c = corners[vid];
    // Pixel position of this corner inside the swapchain.
    let px = u.rect.x + c.x * u.rect.z;
    let py = u.rect.y + c.y * u.rect.w;
    // Map pixels → clip-space [-1, +1]. wgpu has Y-down NDC for fragments
    // but clip space is Y-up, so we flip Y here.
    let cx = (px / u.viewport.x) * 2.0 - 1.0;
    let cy = 1.0 - (py / u.viewport.y) * 2.0;
    var out: VsOut;
    out.clip = vec4<f32>(cx, cy, 0.0, 1.0);
    out.uv = c;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return textureSample(t_image, s_image, in.uv);
}
"#;

/// Per-placement uniform; std140-friendly (vec4 + vec2 + vec2 pad = 32 B).
#[repr(C)]
#[derive(Copy, Clone, Debug)]
struct PlacementUniform {
    rect: [f32; 4],
    viewport: [f32; 2],
    _pad: [f32; 2],
}

unsafe impl bytemuck_compat::Pod for PlacementUniform {}
unsafe impl bytemuck_compat::Zeroable for PlacementUniform {}

/// Local replacement for `bytemuck::{Pod, Zeroable}` so we don't add a
/// dependency. The marker traits are unsafe by design; only
/// [`PlacementUniform`] is annotated here and the implementation is
/// straightforward `#[repr(C)]` with no padding gaps that matter.
mod bytemuck_compat {
    /// Marker for types that can be safely re-interpreted as `&[u8]`.
    ///
    /// # Safety
    ///
    /// Implementors must guarantee that the type has a defined, padding-free
    /// byte representation (`#[repr(C)]` or equivalent) and that every bit
    /// pattern is a valid instance of the type.
    pub unsafe trait Pod: Copy + 'static {}
    /// Marker for types whose all-zero bit pattern is a valid value.
    ///
    /// # Safety
    ///
    /// Implementors must guarantee that an all-zero byte pattern produces a
    /// valid instance of the type (no non-nullable references / niches).
    pub unsafe trait Zeroable: Sized {}

    /// Re-interpret a `Pod` slice as bytes.
    pub fn cast_slice<T: Pod>(slice: &[T]) -> &[u8] {
        let len_bytes = std::mem::size_of_val(slice);
        // SAFETY: `Pod` implementors must be safe to reinterpret as bytes.
        unsafe { std::slice::from_raw_parts(slice.as_ptr() as *const u8, len_bytes) }
    }
}

/// The wgpu pipeline + reusable sampler.
pub struct OverlayPipeline {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    surface_format: wgpu::TextureFormat,
}

impl OverlayPipeline {
    pub fn new(device: &wgpu::Device, surface_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("native-poc-image-overlay-shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER_SRC.into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("native-poc-image-overlay-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
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
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("native-poc-image-overlay-pl"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("native-poc-image-overlay-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[],
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
                            src_factor: wgpu::BlendFactor::SrcAlpha,
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
            label: Some("native-poc-image-overlay-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        Self {
            pipeline,
            bind_group_layout,
            sampler,
            surface_format,
        }
    }

    pub fn surface_format(&self) -> wgpu::TextureFormat {
        self.surface_format
    }

    /// Prepare a per-placement bind-group set for every currently-visible
    /// placement. Caller passes the result to [`draw`] inside the render
    /// pass. Splitting build vs draw lets the bind groups outlive the
    /// `RenderPass` lifetime (`'pass`).
    pub fn build_frame(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layer: &ImageLayer,
        viewport_w: u32,
        viewport_h: u32,
    ) -> Vec<DrawCommand> {
        if viewport_w == 0 || viewport_h == 0 {
            return Vec::new();
        }
        let placements = layer.resolve_placements();
        let mut out = Vec::with_capacity(placements.len());
        for p in placements {
            let tex = match layer.textures.get(&p.image_id) {
                Some(t) => t,
                None => continue,
            };
            let uniform = PlacementUniform {
                rect: [
                    p.pixel_x as f32,
                    p.pixel_y as f32,
                    p.pixel_w as f32,
                    p.pixel_h as f32,
                ],
                viewport: [viewport_w as f32, viewport_h as f32],
                _pad: [0.0, 0.0],
            };
            let buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("native-poc-image-overlay-uniform"),
                size: std::mem::size_of::<PlacementUniform>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            queue.write_buffer(&buf, 0, bytemuck_compat::cast_slice(&[uniform]));
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("native-poc-image-overlay-bg"),
                layout: &self.bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: buf.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&tex.view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            });
            out.push(DrawCommand {
                _uniform_buf: buf,
                bind_group,
            });
        }
        out
    }

    /// Render the per-placement bind groups produced by [`build_frame`].
    /// The render pass must already be configured with `LoadOp::Load`.
    pub fn draw<'pass>(
        &'pass self,
        rpass: &mut wgpu::RenderPass<'pass>,
        commands: &'pass [DrawCommand],
    ) {
        if commands.is_empty() {
            return;
        }
        rpass.set_pipeline(&self.pipeline);
        for cmd in commands {
            rpass.set_bind_group(0, &cmd.bind_group, &[]);
            rpass.draw(0..4, 0..1);
        }
    }
}

/// Drawable record produced by [`OverlayPipeline::build_frame`]; consumed
/// by [`OverlayPipeline::draw`]. The buffer is held alive so the bind
/// group entry that points into it stays valid for the render-pass
/// lifetime. wgpu bind groups internally Arc their texture views, so we
/// do not need to keep the source [`ImageLayer`] alive beyond the call
/// that built the commands.
pub struct DrawCommand {
    _uniform_buf: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placement_uniform_size_is_32_bytes() {
        // std140 alignment expects vec4 + vec2 + vec2 = 32 bytes.
        assert_eq!(std::mem::size_of::<PlacementUniform>(), 32);
    }

    #[test]
    fn cast_slice_returns_pod_byte_view() {
        let u = [PlacementUniform {
            rect: [1.0, 2.0, 3.0, 4.0],
            viewport: [800.0, 600.0],
            _pad: [0.0, 0.0],
        }];
        let bytes = bytemuck_compat::cast_slice(&u);
        assert_eq!(bytes.len(), 32);
    }

    #[test]
    fn shader_source_is_non_empty_wgsl() {
        // Catch accidental shader deletion in a refactor.
        assert!(SHADER_SRC.contains("@vertex"));
        assert!(SHADER_SRC.contains("@fragment"));
    }
}
