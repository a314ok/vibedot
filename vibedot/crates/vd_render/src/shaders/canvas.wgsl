// Canvas "ubershader": sprites, anti-aliased SDF shapes and SDF text,
// all drawn as instanced quads. One draw call per (texture, blend) batch.

struct Globals {
    view: mat4x4<f32>,
    // x: time, y: text emboldening offset, z: encode sRGB (HUD), w: unused
    params: vec4<f32>,
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var t_tex: texture_2d<f32>;
@group(1) @binding(1) var t_samp: sampler;

struct Inst {
    // Affine 2x2 part: x_axis.xy, y_axis.xy
    @location(0) m: vec4<f32>,
    // translation.xy, size.xy (local pixels)
    @location(1) t_size: vec4<f32>,
    @location(2) uv: vec4<f32>,
    @location(3) color: vec4<f32>,
    // shape, corner radius, thickness, softness
    @location(4) params: vec4<f32>,
    // emission, additive, 0, 0
    @location(5) extra: vec4<f32>,
};

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) local: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) @interpolate(flat) params: vec4<f32>,
    @location(4) @interpolate(flat) size_em: vec4<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, inst: Inst) -> VOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-0.5, -0.5), vec2<f32>(0.5, -0.5), vec2<f32>(0.5, 0.5),
        vec2<f32>(-0.5, -0.5), vec2<f32>(0.5, 0.5), vec2<f32>(-0.5, 0.5),
    );
    let c = corners[vi];
    let size = inst.t_size.zw;
    // Grow SDF quads so soft edges and anti-aliasing are never clipped.
    var grow = vec2<f32>(1.0, 1.0);
    if (inst.params.x > 0.5 && inst.params.x < 4.5) {
        let pad = inst.params.w + 2.0;
        grow = (abs(size) + vec2<f32>(2.0 * pad)) / max(abs(size), vec2<f32>(0.001));
    }
    let lc = c * grow;
    let world = vec2<f32>(inst.m.x * lc.x + inst.m.z * lc.y, inst.m.y * lc.x + inst.m.w * lc.y) + inst.t_size.xy;
    var out: VOut;
    out.clip = g.view * vec4<f32>(world, 0.0, 1.0);
    out.uv = mix(inst.uv.xy, inst.uv.zw, lc + vec2<f32>(0.5));
    out.local = lc * abs(size);
    out.color = inst.color;
    out.params = inst.params;
    out.size_em = vec4<f32>(abs(size), inst.extra.x, inst.extra.y);
    return out;
}

fn sd_round_box(p: vec2<f32>, half: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - half + vec2<f32>(r);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

// Exact signed distance to a triangle (iq).
fn sd_triangle(p: vec2<f32>, p0: vec2<f32>, p1: vec2<f32>, p2: vec2<f32>) -> f32 {
    let e0 = p1 - p0;
    let e1 = p2 - p1;
    let e2 = p0 - p2;
    let v0 = p - p0;
    let v1 = p - p1;
    let v2 = p - p2;
    let pq0 = v0 - e0 * clamp(dot(v0, e0) / dot(e0, e0), 0.0, 1.0);
    let pq1 = v1 - e1 * clamp(dot(v1, e1) / dot(e1, e1), 0.0, 1.0);
    let pq2 = v2 - e2 * clamp(dot(v2, e2) / dot(e2, e2), 0.0, 1.0);
    let s = sign(e0.x * e2.y - e0.y * e2.x);
    let d = min(
        min(vec2<f32>(dot(pq0, pq0), s * (v0.x * e0.y - v0.y * e0.x)), vec2<f32>(dot(pq1, pq1), s * (v1.x * e1.y - v1.y * e1.x))),
        vec2<f32>(dot(pq2, pq2), s * (v2.x * e2.y - v2.y * e2.x)),
    );
    return -sqrt(d.x) * sign(d.y);
}

fn shade(in: VOut) -> vec4<f32> {
    // Sample unconditionally: derivatives must stay in uniform control flow.
    let tex = textureSample(t_tex, t_samp, in.uv);
    let shape = i32(in.params.x + 0.5);
    let half = in.size_em.xy * 0.5;
    let p = in.local;
    var d = -1.0;
    switch shape {
        case 1: {
            d = sd_round_box(p, half, min(in.params.y, min(half.x, half.y)));
        }
        case 2: {
            d = length(p) - min(half.x, half.y);
        }
        case 3: {
            let r = min(half.x, half.y);
            let t = max(in.params.z, 0.5);
            d = abs(length(p) - r + t * 0.5) - t * 0.5;
        }
        case 4: {
            // Points toward +x (the "forward" direction of rotation 0).
            d = sd_triangle(p, vec2<f32>(half.x, 0.0), vec2<f32>(-half.x, -half.y), vec2<f32>(-half.x, half.y));
        }
        case 5: {
            // SDF text: atlas alpha 0.5 = edge.
            d = (0.5 - g.params.y) - tex.a;
        }
        default: {}
    }
    let aa = max(fwidth(d), 0.0001);
    let soft = in.params.w;
    var col = in.color;
    var cov = 1.0;
    if (shape == 0) {
        col = col * tex;
    } else if (shape == 5) {
        cov = 1.0 - smoothstep(-aa * 0.7, aa * 0.7, d);
    } else {
        cov = 1.0 - smoothstep(-aa * 0.5 - soft, aa * 0.5, d);
    }
    return vec4<f32>(col.rgb, col.a * cov);
}

struct SceneOut {
    @location(0) albedo: vec4<f32>,
    @location(1) emissive: vec4<f32>,
};

@fragment
fn fs_scene(in: VOut) -> SceneOut {
    let c = shade(in);
    let em = in.size_em.z;
    var out: SceneOut;
    if (in.size_em.w > 0.5) {
        // Additive: unlit glow, only feeds the emissive buffer.
        out.albedo = vec4<f32>(0.0);
        out.emissive = vec4<f32>(c.rgb * c.a * (1.0 + em), 0.0);
    } else {
        out.albedo = c;
        out.emissive = vec4<f32>(c.rgb * em, c.a);
    }
    return out;
}

fn encode_srgb(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(max(c, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}

@fragment
fn fs_hud(in: VOut) -> @location(0) vec4<f32> {
    var c = shade(in);
    // Non-sRGB targets store display-encoded values (params.z = 1).
    if (g.params.z > 0.5) {
        c = vec4<f32>(encode_srgb(clamp(c.rgb, vec3<f32>(0.0), vec3<f32>(1.0))), c.a);
    }
    if (in.size_em.w > 0.5) {
        return vec4<f32>(c.rgb * c.a, 0.0);
    }
    return c;
}

// ---------------------------------------------------------------- lights

struct LInst {
    // position.xy, radius, falloff
    @location(0) pos_r: vec4<f32>,
    @location(1) color: vec4<f32>,
};

struct LOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) @interpolate(flat) color: vec4<f32>,
    @location(2) @interpolate(flat) falloff: f32,
};

@vertex
fn vs_light(@builtin(vertex_index) vi: u32, l: LInst) -> LOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0),
    );
    let c = corners[vi];
    var out: LOut;
    out.clip = g.view * vec4<f32>(l.pos_r.xy + c * l.pos_r.z, 0.0, 1.0);
    out.local = c;
    out.color = l.color;
    out.falloff = l.pos_r.w;
    return out;
}

@fragment
fn fs_light(in: LOut) -> @location(0) vec4<f32> {
    let d = length(in.local);
    let att = pow(clamp(1.0 - d, 0.0, 1.0), in.falloff);
    return vec4<f32>(in.color.rgb * att, 1.0);
}
