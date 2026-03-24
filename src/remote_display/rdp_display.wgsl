// Fullscreen-quad vertex + RGBA texture sampler for RDP display.
// Uses a 3-vertex trick: vertex_index 0,1,2 → triangle covering the entire clip space.

struct VertexOutput {
    @builtin(position) position: vec4f,
    @location(0) uv: vec2f,
};

@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> VertexOutput {
    // Full-screen triangle: (−1,−1), (3,−1), (−1,3)
    let x = f32(i32(idx & 1u) * 4 - 1);
    let y = f32(i32(idx >> 1u) * 4 - 1);
    var out: VertexOutput;
    out.position = vec4f(x, y, 0.0, 1.0);
    // Map clip coords to UV: x∈[−1,3]→[0,2], y∈[−1,3]→[0,2] (clamped by sampler)
    out.uv = vec2f((x + 1.0) * 0.5, (1.0 - y) * 0.5);
    return out;
}

@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var tex_sampler: sampler;

// Uniforms: viewport size and texture size for aspect-ratio–correct rendering.
struct Uniforms {
    viewport: vec2f,
    tex_size: vec2f,
};
@group(0) @binding(2) var<uniform> u: Uniforms;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4f {
    // Compute aspect-ratio–preserving UV (Contain fit)
    let vp_aspect = u.viewport.x / u.viewport.y;
    let tex_aspect = u.tex_size.x / u.tex_size.y;

    var scale: vec2f;
    if tex_aspect > vp_aspect {
        // Texture wider than viewport → letterbox top/bottom
        scale = vec2f(1.0, vp_aspect / tex_aspect);
    } else {
        // Texture taller → pillarbox left/right
        scale = vec2f(tex_aspect / vp_aspect, 1.0);
    }

    let centered_uv = (in.uv - 0.5) / scale + 0.5;

    // Outside texture bounds → black
    if centered_uv.x < 0.0 || centered_uv.x > 1.0 || centered_uv.y < 0.0 || centered_uv.y > 1.0 {
        return vec4f(0.0, 0.0, 0.0, 1.0);
    }

    return textureSample(tex, tex_sampler, centered_uv);
}
