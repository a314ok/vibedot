// Full-screen passes: lighting composite, bloom pyramid and final post FX.

struct Post {
    // exposure, tonemap mode, bloom intensity, bloom threshold
    a: vec4<f32>,
    // bloom radius, vignette, chromatic aberration, film grain
    b: vec4<f32>,
    // scanlines, crt curvature, saturation, contrast
    c: vec4<f32>,
    tint: vec4<f32>,
    flash: vec4<f32>,
    // time, pixelate, output needs manual sRGB encode, bloom enabled
    d: vec4<f32>,
    // 1/src_w, 1/src_h, dst_w, dst_h
    texel: vec4<f32>,
    // ambient light color
    ambient: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Post;
@group(0) @binding(1) var s_lin: sampler;
@group(0) @binding(2) var t0: texture_2d<f32>;
@group(0) @binding(3) var t1: texture_2d<f32>;
@group(0) @binding(4) var t2: texture_2d<f32>;

struct FsIn {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// One oversized triangle covering the screen.
@vertex
fn vs_fullscreen(@builtin(vertex_index) vi: u32) -> FsIn {
    let x = f32(i32(vi & 1u) * 4 - 1);
    let y = f32(i32(vi >> 1u) * 4 - 1);
    var out: FsIn;
    out.pos = vec4<f32>(x, y, 0.0, 1.0);
    out.uv = vec2<f32>((x + 1.0) * 0.5, (1.0 - y) * 0.5);
    return out;
}

// ---------------------------------------------------------------- composite

@fragment
fn fs_composite(in: FsIn) -> @location(0) vec4<f32> {
    let albedo = textureSample(t0, s_lin, in.uv);
    let emissive = textureSample(t1, s_lin, in.uv);
    let light = textureSample(t2, s_lin, in.uv);
    return vec4<f32>(albedo.rgb * light.rgb + emissive.rgb, 1.0);
}

// ---------------------------------------------------------------- bloom

// 13-tap downsample (Jimenez, "Next Generation Post Processing in CoD:AW").
fn downsample13(tex: texture_2d<f32>, uv: vec2<f32>, t: vec2<f32>) -> vec3<f32> {
    let a = textureSample(tex, s_lin, uv + t * vec2<f32>(-2.0, -2.0)).rgb;
    let b = textureSample(tex, s_lin, uv + t * vec2<f32>(0.0, -2.0)).rgb;
    let c = textureSample(tex, s_lin, uv + t * vec2<f32>(2.0, -2.0)).rgb;
    let d = textureSample(tex, s_lin, uv + t * vec2<f32>(-2.0, 0.0)).rgb;
    let e = textureSample(tex, s_lin, uv).rgb;
    let f = textureSample(tex, s_lin, uv + t * vec2<f32>(2.0, 0.0)).rgb;
    let g = textureSample(tex, s_lin, uv + t * vec2<f32>(-2.0, 2.0)).rgb;
    let h = textureSample(tex, s_lin, uv + t * vec2<f32>(0.0, 2.0)).rgb;
    let i = textureSample(tex, s_lin, uv + t * vec2<f32>(2.0, 2.0)).rgb;
    let j = textureSample(tex, s_lin, uv + t * vec2<f32>(-1.0, -1.0)).rgb;
    let k = textureSample(tex, s_lin, uv + t * vec2<f32>(1.0, -1.0)).rgb;
    let l = textureSample(tex, s_lin, uv + t * vec2<f32>(-1.0, 1.0)).rgb;
    let m = textureSample(tex, s_lin, uv + t * vec2<f32>(1.0, 1.0)).rgb;
    var o = e * 0.125;
    o += (a + c + g + i) * 0.03125;
    o += (b + d + f + h) * 0.0625;
    o += (j + k + l + m) * 0.125;
    return o;
}

@fragment
fn fs_bloom_prefilter(in: FsIn) -> @location(0) vec4<f32> {
    var c = downsample13(t0, in.uv, u.texel.xy);
    // Clamp fireflies, then soft-knee threshold.
    c = min(c, vec3<f32>(64.0));
    let threshold = u.a.w;
    let knee = max(threshold * 0.5, 0.0001);
    let br = max(c.r, max(c.g, c.b));
    var soft = clamp(br - threshold + knee, 0.0, 2.0 * knee);
    soft = soft * soft / (4.0 * knee + 0.00001);
    let contrib = max(soft, br - threshold) / max(br, 0.00001);
    return vec4<f32>(c * contrib, 1.0);
}

@fragment
fn fs_bloom_down(in: FsIn) -> @location(0) vec4<f32> {
    return vec4<f32>(downsample13(t0, in.uv, u.texel.xy), 1.0);
}

@fragment
fn fs_bloom_up(in: FsIn) -> @location(0) vec4<f32> {
    // 3x3 tent filter; result is added onto the larger mip (blend One+One).
    let t = u.texel.xy * u.b.x;
    var s = textureSample(t0, s_lin, in.uv + vec2<f32>(-t.x, -t.y)).rgb;
    s += textureSample(t0, s_lin, in.uv + vec2<f32>(0.0, -t.y)).rgb * 2.0;
    s += textureSample(t0, s_lin, in.uv + vec2<f32>(t.x, -t.y)).rgb;
    s += textureSample(t0, s_lin, in.uv + vec2<f32>(-t.x, 0.0)).rgb * 2.0;
    s += textureSample(t0, s_lin, in.uv).rgb * 4.0;
    s += textureSample(t0, s_lin, in.uv + vec2<f32>(t.x, 0.0)).rgb * 2.0;
    s += textureSample(t0, s_lin, in.uv + vec2<f32>(-t.x, t.y)).rgb;
    s += textureSample(t0, s_lin, in.uv + vec2<f32>(0.0, t.y)).rgb * 2.0;
    s += textureSample(t0, s_lin, in.uv + vec2<f32>(t.x, t.y)).rgb;
    return vec4<f32>(s / 16.0, 1.0);
}

// ---------------------------------------------------------------- final

fn tonemap_aces(x: vec3<f32>) -> vec3<f32> {
    // Narkowicz 2015 fit.
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;
    return clamp((x * (a * x + b)) / (x * (c * x + d) + e), vec3<f32>(0.0), vec3<f32>(1.0));
}

fn agx_contrast(x: vec3<f32>) -> vec3<f32> {
    let x2 = x * x;
    let x4 = x2 * x2;
    return 15.5 * x4 * x2 - 40.14 * x4 * x + 31.96 * x4 - 6.868 * x2 * x + 0.4298 * x2 + 0.1191 * x - 0.00232;
}

fn tonemap_agx(c: vec3<f32>) -> vec3<f32> {
    let m = mat3x3<f32>(
        vec3<f32>(0.842479062253094, 0.0423282422610123, 0.0423756549057051),
        vec3<f32>(0.0784335999999992, 0.878468636469772, 0.0784336),
        vec3<f32>(0.0792237451477643, 0.0791661274605434, 0.879142973793104),
    );
    let inv = mat3x3<f32>(
        vec3<f32>(1.19687900512017, -0.0528968517574562, -0.0529716355144438),
        vec3<f32>(-0.0980208811401368, 1.15190312990417, -0.0980434501171241),
        vec3<f32>(-0.0990297440797205, -0.0989611768448433, 1.15107367264116),
    );
    let min_ev = -12.47393;
    let max_ev = 4.026069;
    var v = m * max(c, vec3<f32>(1e-10));
    v = clamp(log2(v), vec3<f32>(min_ev), vec3<f32>(max_ev));
    v = (v - min_ev) / (max_ev - min_ev);
    v = agx_contrast(v);
    v = inv * v;
    // AgX output is display-referred sRGB-ish; convert back to linear.
    return pow(max(v, vec3<f32>(0.0)), vec3<f32>(2.2));
}

fn tonemap(c: vec3<f32>, mode: f32) -> vec3<f32> {
    if (mode < 0.5) {
        return clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
    } else if (mode < 1.5) {
        return c / (vec3<f32>(1.0) + c);
    } else if (mode < 2.5) {
        return tonemap_aces(c);
    }
    return tonemap_agx(c);
}

fn hash12(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.xyx) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}

fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}

@fragment
fn fs_final(in: FsIn) -> @location(0) vec4<f32> {
    var uv = in.uv;
    // CRT barrel distortion.
    let curv = u.c.y;
    if (curv > 0.0) {
        let cc = uv * 2.0 - 1.0;
        let off = cc.yx * cc.yx * curv;
        uv = (cc + cc * off) * 0.5 + 0.5;
    }
    let outside = uv.x < 0.0 || uv.y < 0.0 || uv.x > 1.0 || uv.y > 1.0;
    // Pixelate: snap to the centers of the (low-res) source texels.
    let src_size = 1.0 / u.texel.xy;
    var suv = uv;
    if (u.d.y > 1.0) {
        suv = (floor(uv * src_size) + 0.5) / src_size;
    }
    // Chromatic aberration grows toward the edges.
    let ca = u.b.z * 0.0015;
    let dir = (suv - 0.5) * ca;
    var hdr = vec3<f32>(
        textureSample(t0, s_lin, suv + dir).r,
        textureSample(t0, s_lin, suv).g,
        textureSample(t0, s_lin, suv - dir).b,
    );
    let bloom = textureSample(t1, s_lin, uv).rgb;
    if (u.d.w > 0.5) {
        hdr += bloom * u.a.z;
    }
    hdr *= u.a.x;
    var c = tonemap(hdr, u.a.y);
    // Grading: contrast around mid grey, saturation, tint.
    c = (c - 0.18) * u.c.w + 0.18;
    let luma = dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
    c = mix(vec3<f32>(luma), c, u.c.z);
    c = max(c * u.tint.rgb, vec3<f32>(0.0));
    // Flash overlay.
    c = mix(c, u.flash.rgb, clamp(u.flash.a, 0.0, 1.0));
    // Vignette.
    let vd = distance(uv, vec2<f32>(0.5));
    c *= 1.0 - smoothstep(0.35, 0.95, vd) * u.b.y;
    // Scanlines (in output pixels).
    if (u.c.x > 0.0) {
        let line = 0.5 + 0.5 * sin(in.pos.y * 3.14159);
        c *= 1.0 - u.c.x * 0.5 * (1.0 - line);
    }
    // Film grain.
    if (u.b.w > 0.0) {
        let n = hash12(in.pos.xy + fract(u.d.x * 7.13) * 1000.0) - 0.5;
        c += vec3<f32>(n * u.b.w);
    }
    c = clamp(c, vec3<f32>(0.0), vec3<f32>(1.0));
    if (outside) {
        c = vec3<f32>(0.0);
    }
    if (u.d.z > 0.5) {
        c = linear_to_srgb(c);
    }
    return vec4<f32>(c, 1.0);
}
