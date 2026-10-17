struct Controls {
    pixel: vec4<f32>,
    grade: vec4<f32>,
    display: vec4<f32>,
    inverse_view_projection: mat4x4<f32>,
    view_projection: mat4x4<f32>,
    // xyz: camera position, w: ground wetness (0 turns reflections off).
    eye: vec4<f32>,
    sky: vec4<f32>,
};
@group(0) @binding(0) var scene: texture_2d<f32>;
@group(0) @binding(1) var linear_sampler: sampler;
@group(0) @binding(2) var<uniform> controls: Controls;
@group(0) @binding(3) var scene_depth: texture_depth_2d;
struct Screen { @builtin(position) position: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs_main(@builtin(vertex_index) index: u32) -> Screen {
    let p = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    var out: Screen;
    out.position = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(p.x, 1.0 - p.y);
    return out;
}
fn read(uv: vec2<f32>) -> vec3<f32> { return textureSampleLevel(scene, linear_sampler, uv, 0.0).rgb; }
fn luma(c: vec3<f32>) -> f32 { return dot(c, vec3<f32>(0.299, 0.587, 0.114)); }
fn bright(c: vec3<f32>) -> vec3<f32> { return c * smoothstep(0.65, 1.0, luma(c)); }
// Enhanced lighting: wet ground reflects the finished scene by marching the
// reflected view ray through the depth buffer.
fn depth_at(uv: vec2<f32>) -> f32 {
    let size = vec2<i32>(textureDimensions(scene_depth));
    let texel = clamp(vec2<i32>(uv * vec2<f32>(size)), vec2<i32>(0), size - 1);
    return textureLoad(scene_depth, texel, 0);
}
fn world_at(uv: vec2<f32>, depth: f32) -> vec3<f32> {
    let ndc = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, depth, 1.0);
    let world = controls.inverse_view_projection * ndc;
    return world.xyz / world.w;
}
// Must match `puddles` in shader.wgsl so the darkened ground and its
// reflection line up.
fn hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}
fn noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash(i), hash(i + vec2<f32>(1.0, 0.0)), u.x), mix(hash(i + vec2<f32>(0.0, 1.0)), hash(i + vec2<f32>(1.0, 1.0)), u.x), u.y);
}
fn puddles(p: vec2<f32>, wetness: f32) -> f32 {
    let n = noise(p * 0.18) * 0.65 + noise(p * 0.7) * 0.35;
    let pools = smoothstep(0.55 - wetness * 0.2, 0.7 - wetness * 0.2, n);
    return wetness * mix(0.35, 1.0, pools);
}
fn reflection(uv: vec2<f32>, color: vec3<f32>) -> vec3<f32> {
    let wetness = controls.eye.w;
    let depth = depth_at(uv);
    if (wetness <= 0.0 || depth <= 0.0) { return color; }
    let world = world_at(uv, depth);
    let px = controls.pixel.xy;
    let right = world_at(uv + vec2<f32>(px.x, 0.0), depth_at(uv + vec2<f32>(px.x, 0.0)));
    let down = world_at(uv + vec2<f32>(0.0, px.y), depth_at(uv + vec2<f32>(0.0, px.y)));
    var normal = normalize(cross(down - world, right - world));
    if (normal.y < 0.0) { normal = -normal; }
    let distance = length(world - controls.eye.xyz);
    let wet = puddles(world.xz, wetness) * smoothstep(0.85, 0.97, normal.y) * (1.0 - smoothstep(120.0, 220.0, distance));
    if (wet <= 0.01) { return color; }
    // Puddles are flat even where the face normal is a little off.
    let view = (world - controls.eye.xyz) / distance;
    let ray = reflect(view, vec3<f32>(0.0, 1.0, 0.0));
    let fresnel = 0.02 + 0.98 * pow(1.0 - max(-view.y, 0.0), 5.0);
    var found = controls.sky.rgb;
    var step = 0.25 + distance * 0.01;
    var travelled = 0.0;
    var previous = 0.0;
    for (var i = 0; i < 32; i += 1) {
        travelled += step;
        let point = world + ray * travelled;
        let clip = controls.view_projection * vec4<f32>(point, 1.0);
        if (clip.w <= 0.0) { break; }
        let sample_uv = vec2<f32>(clip.x / clip.w * 0.5 + 0.5, 0.5 - clip.y / clip.w * 0.5);
        if (any(sample_uv < vec2<f32>(0.0)) || any(sample_uv > vec2<f32>(1.0))) { break; }
        let scene_depth_here = depth_at(sample_uv);
        if (scene_depth_here <= 0.0) {
            // Open sky above the puddle.
            found = read(sample_uv);
            previous = travelled;
            step *= 1.2;
            continue;
        }
        let behind = length(point - controls.eye.xyz) - length(world_at(sample_uv, scene_depth_here) - controls.eye.xyz);
        if (behind > 0.0 && behind < step * 2.0 + 0.5) {
            // Refine between the last miss and this hit.
            var low = previous;
            var high = travelled;
            for (var j = 0; j < 5; j += 1) {
                let middle = (low + high) * 0.5;
                let c = controls.view_projection * vec4<f32>(world + ray * middle, 1.0);
                let m_uv = vec2<f32>(c.x / c.w * 0.5 + 0.5, 0.5 - c.y / c.w * 0.5);
                let m_behind = length(world + ray * middle - controls.eye.xyz) - length(world_at(m_uv, depth_at(m_uv)) - controls.eye.xyz);
                if (m_behind > 0.0) { high = middle; } else { low = middle; }
            }
            let c = controls.view_projection * vec4<f32>(world + ray * high, 1.0);
            let hit_uv = vec2<f32>(c.x / c.w * 0.5 + 0.5, 0.5 - c.y / c.w * 0.5);
            // Fade towards the screen edges, where the reflection would cut off.
            let edge = min(min(hit_uv.x, 1.0 - hit_uv.x), min(hit_uv.y, 1.0 - hit_uv.y));
            found = mix(found, read(hit_uv), smoothstep(0.0, 0.08, edge));
            break;
        }
        previous = travelled;
        step *= 1.2;
    }
    return mix(color, found, clamp(fresnel * wet, 0.0, 0.85));
}
@fragment fn fs_main(input: Screen) -> @location(0) vec4<f32> {
    let uv = input.uv;
    let px = controls.pixel.xy;
    let center = read(uv);
    let north = read(uv - vec2<f32>(0.0, px.y));
    let south = read(uv + vec2<f32>(0.0, px.y));
    let east = read(uv + vec2<f32>(px.x, 0.0));
    let west = read(uv - vec2<f32>(px.x, 0.0));
    var color = center;
    if (controls.pixel.w > 0.5) {
        let nw = luma(read(uv + px * vec2<f32>(-1.0, -1.0)));
        let ne = luma(read(uv + px * vec2<f32>(1.0, -1.0)));
        let sw = luma(read(uv + px * vec2<f32>(-1.0, 1.0)));
        let se = luma(read(uv + px));
        let m = luma(center);
        let low = min(m, min(min(nw, ne), min(sw, se)));
        let high = max(m, max(max(nw, ne), max(sw, se)));
        if (high - low > max(0.0312, high * 0.125)) {
            var direction = vec2<f32>(-((nw + ne) - (sw + se)), (nw + sw) - (ne + se));
            let reduce = max((nw + ne + sw + se) * 0.03125, 0.0078125);
            direction = clamp(direction / (min(abs(direction.x), abs(direction.y)) + reduce), vec2<f32>(-8.0), vec2<f32>(8.0)) * px;
            let a = 0.5 * (read(uv + direction * (-1.0 / 6.0)) + read(uv + direction * (1.0 / 6.0)));
            let b = a * 0.5 + 0.25 * (read(uv - direction * 0.5) + read(uv + direction * 0.5));
            color = select(b, a, luma(b) < low || luma(b) > high);
        }
    }
    // Clamp the unsharp mask to the local range to avoid ringing and bright halos.
    let detail = center - (north + south + east + west) * 0.25;
    let local_min = min(center, min(min(north, south), min(east, west)));
    let local_max = max(center, max(max(north, south), max(east, west)));
    let sharpness = select(controls.pixel.z, 0.0, controls.display.z > 0.5);
    color = clamp(color + detail * sharpness, local_min, local_max);
    if (controls.eye.w > 0.0) {
        color = reflection(uv, color);
    }
    if (controls.grade.x > 0.0) {
        var glow = bright(center) * 0.2;
        for (var i = 0u; i < 8u; i += 1u) {
            let angle = f32(i) * 0.78539816;
            let offset = vec2<f32>(cos(angle), sin(angle)) * controls.display.xy * 12.0;
            glow += bright(read(uv + offset)) * 0.1;
        }
        color += glow * controls.grade.x;
    }
    color *= controls.grade.y;
    color = mix(vec3<f32>(luma(color)), color, controls.grade.z);
    // Filmic display mapping; output is linear and the sRGB target handles encoding.
    color = clamp((color * (2.51 * color + 0.03)) / (color * (2.43 * color + 0.59) + 0.14), vec3<f32>(0.0), vec3<f32>(1.0));
    let edge = dot(uv - 0.5, uv - 0.5) * 2.0;
    color *= 1.0 - edge * controls.grade.w;
    return vec4<f32>(color, 1.0);
}
