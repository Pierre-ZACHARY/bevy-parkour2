// Dotted background for graph canvases: a dot at every grid crossing,
// panned and zoomed with the canvas.
import bevy_ui_render::ui_vertex_output::UiVertexOutput;

struct Dots {
    background: vec4<f32>,
    dot: vec4<f32>,
    // xy: pan, z: zoom, w: spacing in graph units.
    view: vec4<f32>,
    // xy: canvas size in logical pixels, z: dot radius in pixels.
    extent: vec4<f32>,
}
@group(1) @binding(0) var<uniform> dots: Dots;

@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    let p = in.uv * dots.extent.xy;
    let local = p - dots.view.xy;
    let pixel = max(length(fwidth(p)) * 0.7071, 1e-4);
    let spacing = dots.view.w * dots.view.z;
    let cell = local - spacing * round(local / spacing);
    let radius = dots.extent.z;
    // Anti-aliased disc, faded out as dots get too dense to read.
    let coverage = (1.0 - smoothstep(radius - 0.5 * pixel, radius + 0.5 * pixel, length(cell)))
        * smoothstep(5.0, 12.0, spacing);
    let a = dots.dot.a * coverage;
    let out_a = a + dots.background.a * (1.0 - a);
    let rgb = (dots.dot.rgb * a + dots.background.rgb * dots.background.a * (1.0 - a)) / max(out_a, 1e-5);
    return vec4<f32>(rgb, out_a);
}
