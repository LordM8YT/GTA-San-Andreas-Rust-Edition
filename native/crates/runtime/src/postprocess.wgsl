struct Controls { pixel: vec4<f32>, grade: vec4<f32>, display: vec4<f32> };
@group(0) @binding(0) var scene: texture_2d<f32>;
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
fn read(uv: vec2<f32>) -> vec3<f32> { return textureSampleLevel(scene, linear_sampler, uv, 0.0).rgb; }
fn luma(c: vec3<f32>) -> f32 { return dot(c, vec3<f32>(0.299, 0.587, 0.114)); }
fn bright(c: vec3<f32>) -> vec3<f32> { return c * smoothstep(0.65, 1.0, luma(c)); }
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
