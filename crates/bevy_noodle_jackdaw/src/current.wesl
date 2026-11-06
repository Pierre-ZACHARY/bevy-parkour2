// Dashed outline of a rounded rectangle, the dashes running clockwise
// around it with the global time: the ring around a machine's current state.
import bevy_render::globals::Globals;
import bevy_ui_render::ui_vertex_output::UiVertexOutput;

@group(0) @binding(1) var<uniform> globals: Globals;
struct Ring {
    color: vec4<f32>,
    // xy: the ring node's size in logical pixels, z: inset of the stroke's
    // centre line from the node's edge, w: its corner radius.
    shape: vec4<f32>,
    // x: stroke width, y: dash length, z: dash and gap, w: speed in pixels
    // per second.
    pattern: vec4<f32>,
}
@group(1) @binding(0) var<uniform> ring: Ring;

const QUARTER: f32 = 1.5707963;

@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    let size = ring.shape.xy;
    let half = 0.5 * size - vec2<f32>(ring.shape.z);
    let r = clamp(ring.shape.w, 0.0, min(half.x, half.y));
    // The straight sides' half lengths.
    let q = half - vec2<f32>(r);
    // Centred, y down.
    let p = in.uv * size - 0.5 * size;

    let d = abs(p) - q;
    let distance = abs(length(max(d, vec2<f32>(0.0))) + min(max(d.x, d.y), 0.0) - r);

    // How far round the outline the closest point is, clockwise from the
    // top side's left end.
    let lx = 2.0 * q.x;
    let ly = 2.0 * q.y;
    let arc = r * QUARTER;
    var along = 0.0;
    if d.x > 0.0 && d.y > 0.0 {
        // A corner.
        let corner = vec2<f32>(select(-q.x, q.x, p.x > 0.0), select(-q.y, q.y, p.y > 0.0));
        let angle = atan2(p.y - corner.y, p.x - corner.x);
        if p.x > 0.0 && p.y < 0.0 {
            along = lx + (angle + QUARTER) * r;
        } else if p.x > 0.0 {
            along = lx + arc + ly + angle * r;
        } else if p.y > 0.0 {
            along = 2.0 * lx + 2.0 * arc + ly + (angle - QUARTER) * r;
        } else {
            along = 2.0 * lx + 3.0 * arc + 2.0 * ly + (angle + 2.0 * QUARTER) * r;
        }
    } else if d.y > d.x {
        // The top or bottom side.
        along = select(lx + 2.0 * arc + ly + (q.x - p.x), p.x + q.x, p.y < 0.0);
    } else {
        // The right or left side.
        along = select(2.0 * lx + 3.0 * arc + ly + (q.y - p.y), lx + arc + (p.y + q.y), p.x > 0.0);
    }

    // A whole number of dashes round the outline, so none is cut where it
    // starts.
    let total = 2.0 * (lx + ly) + 4.0 * arc;
    let period = total / max(round(total / ring.pattern.z), 1.0);
    let dash = ring.pattern.y * period / ring.pattern.z;

    let pixel = max(length(fwidth(p)) * 0.7071, 1e-4);
    let half_width = max(ring.pattern.x * 0.5, pixel * 0.5);
    var coverage = 1.0 - smoothstep(half_width - pixel, half_width + pixel, distance);
    let phase = along - globals.time * ring.pattern.w;
    let offset = phase - period * floor(phase / period);
    coverage *= clamp(min(offset, dash - offset) / pixel + 0.5, 0.0, 1.0);
    return vec4<f32>(ring.color.rgb, ring.color.a * coverage);
}
