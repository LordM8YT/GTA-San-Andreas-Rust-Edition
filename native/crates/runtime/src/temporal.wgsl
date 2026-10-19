// Temporal anti-aliasing: blends each jittered frame into a reprojected
// history. Motion comes from depth and the camera alone; every vertex is in
// world space, so static scenery reprojects exactly. Moving cars and peds rely
// on the neighbourhood clamp below, which rejects history that disagrees with
// the current frame.
struct Controls {
    // Current (jittered) clip space to world.
    inverse_view_projection: mat4x4<f32>,
    // World to last frame's unjittered clip space.
    previous_view_projection: mat4x4<f32>,
    // xy: 1 / scene size, z: 1 discards history, w: weight of the current frame.
    texel: vec4<f32>,
};
@group(0) @binding(0) var current: texture_2d<f32>;
@group(0) @binding(1) var linear_sampler: sampler;
@group(0) @binding(2) var<uniform> controls: Controls;
@group(0) @binding(3) var depth: texture_depth_2d;
@group(0) @binding(4) var history: texture_2d<f32>;
struct Screen { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs_main(@builtin(vertex_index) index: u32) -> Screen {
    let p = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    var out: Screen;
    out.position = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(p.x, 1.0 - p.y);
    return out;
}
fn luma(c: vec3<f32>) -> f32 { return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722)); }
// Blending in a compressed range keeps a bright sun pixel from smearing.
fn compress(c: vec3<f32>) -> vec3<f32> { return c / (1.0 + luma(c)); }
fn expand(c: vec3<f32>) -> vec3<f32> { return c / max(1.0 - luma(c), 0.0001); }
@fragment fn fs_main(in: Screen) -> @location(0) vec4<f32> {
    let size = vec2<i32>(textureDimensions(current));
    let pixel = vec2<i32>(in.position.xy);
    let center = compress(textureLoad(current, pixel, 0).rgb);
    var mean = vec3<f32>(0.0);
    var square = vec3<f32>(0.0);
    var nearest = pixel;
    var nearest_depth = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let p = clamp(pixel + vec2<i32>(x, y), vec2<i32>(0), size - 1);
            let c = compress(textureLoad(current, p, 0).rgb);
            mean += c;
            square += c * c;
            // Reversed depth: larger is closer. The nearest neighbour's motion
            // keeps object edges from trailing the background's history.
            let d = textureLoad(depth, p, 0);
            if (d > nearest_depth) {
                nearest_depth = d;
                nearest = p;
            }
        }
    }
    if (controls.texel.z > 0.5) {
        return vec4<f32>(expand(center), 1.0);
    }
    mean /= 9.0;
    let deviation = sqrt(max(square / 9.0 - mean * mean, vec3<f32>(0.0)));
    let low = mean - deviation * 1.25;
    let high = mean + deviation * 1.25;
    let nearest_uv = (vec2<f32>(nearest) + 0.5) * controls.texel.xy;
    // The sky has depth zero; a tiny depth reprojects it by rotation only.
    let clip = vec4<f32>(nearest_uv.x * 2.0 - 1.0, 1.0 - nearest_uv.y * 2.0, max(nearest_depth, 0.000001), 1.0);
    let world = controls.inverse_view_projection * clip;
    let previous = controls.previous_view_projection * vec4<f32>(world.xyz / world.w, 1.0);
    if (previous.w <= 0.0) {
        return vec4<f32>(expand(center), 1.0);
    }
    let previous_uv = vec2<f32>(previous.x / previous.w * 0.5 + 0.5, 0.5 - previous.y / previous.w * 0.5);
    let history_uv = in.uv + previous_uv - nearest_uv;
    if (any(history_uv < vec2<f32>(0.0)) || any(history_uv > vec2<f32>(1.0))) {
        return vec4<f32>(expand(center), 1.0);
    }
    let past = clamp(compress(textureSampleLevel(history, linear_sampler, history_uv, 0.0).rgb), low, high);
    return vec4<f32>(expand(mix(past, center, controls.texel.w)), 1.0);
}
