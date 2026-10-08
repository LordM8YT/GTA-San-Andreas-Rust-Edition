@group(0) @binding(0) var source: texture_2d<f32>;

@vertex fn vs_main(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let positions = array<vec2<f32>, 3>(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
    return vec4(positions[index], 0.0, 1.0);
}

@fragment fn fs_main(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let size = textureDimensions(source);
    let destination_size = max(size / 2u, vec2<u32>(1u));
    let pixel = vec2<u32>(position.xy);
    let ratio = vec2<f32>(size) / vec2<f32>(destination_size);
    let low = vec2<f32>(pixel) * ratio;
    let high = vec2<f32>(pixel + 1u) * ratio;
    var sum = vec4<f32>(0.0);
    // Area weights preserve odd-size edge pixels. Texture loads decode sRGB;
    // the sRGB render target encodes the linear, alpha-weighted result.
    for (var y = u32(floor(low.y)); y < u32(ceil(high.y)); y += 1u) {
        for (var x = u32(floor(low.x)); x < u32(ceil(high.x)); x += 1u) {
            let weight = (min(high.x, f32(x + 1u)) - max(low.x, f32(x)))
                       * (min(high.y, f32(y + 1u)) - max(low.y, f32(y)));
            let color = textureLoad(source, vec2<i32>(i32(x), i32(y)), 0);
            sum += vec4(color.rgb * color.a, color.a) * weight;
        }
    }
    if sum.a <= 0.000001 { return vec4<f32>(0.0); }
    return vec4(sum.rgb / sum.a, sum.a / (ratio.x * ratio.y));
}
