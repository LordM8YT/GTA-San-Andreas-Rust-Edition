// WGSL adaptation of AMD FidelityFX FSR 1 FP32 EASU and RCAS.
// Copyright (c) 2021 Advanced Micro Devices, Inc. MIT license.
// See ../vendor/fsr1/license.txt and ffx_fsr1.h for the original algorithms.
// Uses direct clamped loads and exact reciprocals instead of packed gathers
// and approximate intrinsics. Filtering takes place after tone mapping.
struct Controls { pixel: vec4<f32>, grade: vec4<f32>, display: vec4<f32> };
@group(0) @binding(0) var image: texture_2d<f32>;
@group(0) @binding(1) var linear_sampler: sampler;
@group(0) @binding(2) var<uniform> controls: Controls;
struct Screen { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs_main(@builtin(vertex_index) index: u32) -> Screen {
    let p = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    var out: Screen;
    out.position = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(p.x, 1.0 - p.y);
    return out;
}
fn encode(c: vec3<f32>) -> vec3<f32> {
    return select(c * 12.92, 1.055 * pow(max(c, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.4)) - 0.055, c > vec3<f32>(0.0031308));
}
fn decode(c: vec3<f32>) -> vec3<f32> {
    return select(c / 12.92, pow(max((c + 0.055) / 1.055, vec3<f32>(0.0)), vec3<f32>(2.4)), c > vec3<f32>(0.04045));
}
fn load_linear(p: vec2<i32>) -> vec3<f32> {
    return textureLoad(image, clamp(p, vec2<i32>(0), vec2<i32>(textureDimensions(image)) - 1), 0).rgb;
}
fn load_gamma(p: vec2<i32>) -> vec3<f32> { return encode(load_linear(p)); }
fn fsr_luma(c: vec3<f32>) -> f32 { return c.b * 0.5 + c.r * 0.5 + c.g; }
// Returns gradient direction (xy) and edge length (z) for one bilinear corner.
fn easu_set(weight: f32, a: f32, b: f32, c: f32, d: f32, e: f32) -> vec3<f32> {
    let dir = vec2<f32>(d - b, e - a);
    let len = clamp(abs(dir) / max(vec2<f32>(max(abs(d-c), abs(c-b)), max(abs(e-c), abs(c-a))), vec2<f32>(0.000001)), vec2<f32>(0.0), vec2<f32>(1.0));
    return vec3<f32>(dir * weight, dot(len, len) * weight);
}
fn easu_weight(offset: vec2<f32>, direction: vec2<f32>, length: vec2<f32>, lobe: f32) -> f32 {
    let v = vec2<f32>(dot(offset, direction), dot(offset, vec2<f32>(-direction.y, direction.x))) * length;
    let d2 = min(dot(v, v), 1.0 / lobe);
    let base = 0.4 * d2 - 1.0;
    let window = lobe * d2 - 1.0;
    return (1.5625 * base * base - 0.5625) * window * window;
}
@fragment fn fs_easu(input: Screen) -> @location(0) vec4<f32> {
    if (controls.display.z < 0.5) {
        return vec4<f32>(textureSampleLevel(image, linear_sampler, input.uv, 0.0).rgb, 1.0);
    }
    // Native and supersampled output use linear scaling plus RCAS, no EASU.
    if (controls.pixel.x <= controls.display.x) {
        return vec4<f32>(encode(textureSampleLevel(image, linear_sampler, input.uv, 0.0).rgb), 1.0);
    }
    let position = input.position.xy * controls.display.xy / controls.pixel.xy - 0.5;
    let base = vec2<i32>(floor(position));
    let fraction = fract(position);
    let offsets = array<vec2<i32>, 12>(
        vec2<i32>(0,-1), vec2<i32>(1,-1), vec2<i32>(-1,1), vec2<i32>(0,1),
        vec2<i32>(0,0), vec2<i32>(-1,0), vec2<i32>(1,1), vec2<i32>(2,1),
        vec2<i32>(2,0), vec2<i32>(1,0), vec2<i32>(1,2), vec2<i32>(0,2));
    var colors: array<vec3<f32>, 12>;
    var l: array<f32, 12>;
    for (var i = 0u; i < 12u; i += 1u) { colors[i] = load_gamma(base + offsets[i]); l[i] = fsr_luma(colors[i]); }
    var edge = easu_set((1.0-fraction.x)*(1.0-fraction.y), l[0],l[5],l[4],l[9],l[3]);
    edge += easu_set(fraction.x*(1.0-fraction.y), l[1],l[4],l[9],l[8],l[6]);
    edge += easu_set((1.0-fraction.x)*fraction.y, l[4],l[2],l[3],l[6],l[11]);
    edge += easu_set(fraction.x*fraction.y, l[9],l[3],l[6],l[7],l[10]);
    var direction = vec2<f32>(1.0, 0.0);
    if (dot(edge.xy, edge.xy) >= 1.0 / 32768.0) { direction = normalize(edge.xy); }
    let length = (edge.z * 0.5) * (edge.z * 0.5);
    let stretch = dot(direction, direction) / max(abs(direction.x), abs(direction.y));
    let anisotropy = vec2<f32>(1.0 + (stretch - 1.0) * length, 1.0 - 0.5 * length);
    let lobe = 0.5 + (0.21 - 0.5) * length;
    var color = vec3<f32>(0.0);
    var weight = 0.0;
    for (var i = 0u; i < 12u; i += 1u) {
        let w = easu_weight(vec2<f32>(offsets[i]) - fraction, direction, anisotropy, lobe);
        color += colors[i] * w; weight += w;
    }
    let low = min(min(colors[4], colors[9]), min(colors[3], colors[6]));
    let high = max(max(colors[4], colors[9]), max(colors[3], colors[6]));
    return vec4<f32>(clamp(color / max(weight, 0.000001), low, high), 1.0);
}
@fragment fn fs_rcas(input: Screen) -> @location(0) vec4<f32> {
    let p = vec2<i32>(input.position.xy);
    let e = load_linear(p);
    if (controls.display.z < 0.5) { return vec4<f32>(e, 1.0); }
    let b = load_linear(p + vec2<i32>(0,-1));
    let d = load_linear(p + vec2<i32>(-1,0));
    let f = load_linear(p + vec2<i32>(1,0));
    let h = load_linear(p + vec2<i32>(0,1));
    let low = min(min(b,d),min(f,h));
    let high = max(max(b,d),max(f,h));
    let hit_min = min(low,e) / max(4.0 * high, vec3<f32>(0.000001));
    let hit_max = (1.0 - max(high,e)) / min(4.0 * low - 4.0, vec3<f32>(-0.000001));
    let lobes = max(-hit_min,hit_max);
    let lobe = max(-0.1875, min(max(max(lobes.r,lobes.g),lobes.b), 0.0)) * controls.pixel.z;
    let color = (lobe * (b+d+f+h) + e) / (4.0 * lobe + 1.0);
    return vec4<f32>(decode(clamp(color, vec3<f32>(0.0), vec3<f32>(1.0))), 1.0);
}
