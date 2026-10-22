// Zeichnet eine Spur (Bildschirm oder Webcam) als abgerundetes Rechteck mit
// Rahmen und Schatten. Ein Aufruf = ein Rechteck aus vier Ecken.
// Ausgabe mit vormultipliziertem Alpha, gemischt mit „über“.

struct Layer {
    // x, y, Breite, Höhe in Pixeln des Ausgabebilds
    rect: vec4<f32>,
    // Ausschnitt der Quelle, 0 bis 1
    crop: vec4<f32>,
    border_color: vec4<f32>,
    // Grösse des Ausgabebilds
    frame: vec2<f32>,
    radius: f32,
    border: f32,
    shadow: f32,
    opacity: f32,
    mirror: f32,
    // 0 nichts, 1 Hintergrund unscharf, 2 entfernt, 3 entfernt mit Fläche
    effect: f32,
};

@group(0) @binding(0) var<uniform> layer: Layer;
@group(0) @binding(1) var tex: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;
// Maske der Person (R, 1 = Person)
@group(0) @binding(3) var mask: texture_2d<f32>;

// Weichgezeichneter Hintergrund: Scheibe aus Abtastpunkten (Goldener Winkel).
fn blurred(uv: vec2<f32>) -> vec3<f32> {
    let dims = vec2<f32>(textureDimensions(tex));
    let r = 0.04 * layer.crop.z;
    let scale = vec2<f32>(r, r * dims.x / dims.y);
    var sum = vec3<f32>(0.0);
    for (var i = 0u; i < 48u; i = i + 1u) {
        let f = f32(i) + 0.5;
        let rad = sqrt(f / 48.0);
        let ang = f * 2.39996;
        sum = sum + textureSampleLevel(tex, samp, uv + vec2<f32>(cos(ang), sin(ang)) * rad * scale, 0.0).rgb;
    }
    return sum / 48.0;
}

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) px: vec2<f32>,
};

fn blur() -> f32 {
    return 0.05 * min(layer.rect.z, layer.rect.w) + 4.0;
}

@vertex
fn vs(@builtin(vertex_index) i: u32) -> VsOut {
    // Rand für Schatten und Kantenglättung
    var margin = 1.0;
    if (layer.shadow > 0.0) {
        margin = blur() * 2.0;
    }
    let corner = vec2<f32>(f32(i & 1u), f32((i >> 1u) & 1u));
    let p = layer.rect.xy - vec2<f32>(margin) + corner * (layer.rect.zw + vec2<f32>(2.0 * margin));
    var out: VsOut;
    out.pos = vec4<f32>(p.x / layer.frame.x * 2.0 - 1.0, 1.0 - p.y / layer.frame.y * 2.0, 0.0, 1.0);
    out.px = p;
    return out;
}

// Abstand zum Rand eines abgerundeten Rechtecks (negativ = innen).
fn sd_round_rect(p: vec2<f32>, center: vec2<f32>, half: vec2<f32>, r: f32) -> f32 {
    let q = abs(p - center) - half + vec2<f32>(r);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let half = layer.rect.zw * 0.5;
    let center = layer.rect.xy + half;
    let r = min(layer.radius, min(half.x, half.y));
    let d = sd_round_rect(in.px, center, half, r);

    var uv = clamp((in.px - layer.rect.xy) / layer.rect.zw, vec2<f32>(0.0), vec2<f32>(1.0));
    if (layer.mirror > 0.5) {
        uv.x = 1.0 - uv.x;
    }
    let src = layer.crop.xy + uv * layer.crop.zw;
    // Videos sind deckend; Zeiger, Klicks und Ebenen bringen ihre eigene
    // Durchsichtigkeit mit (gerade, nicht vormultipliziert).
    let texel = textureSampleLevel(tex, samp, src, 0.0);
    var color = texel.rgb;
    var person = 1.0;
    if (layer.effect > 0.5) {
        person = textureSampleLevel(mask, samp, src, 0.0).r;
        if (layer.effect < 1.5) {
            color = mix(blurred(src), color, person);
            person = 1.0;
        } else if (layer.effect > 2.5) {
            color = mix(vec3<f32>(0.11, 0.11, 0.12), color, person);
            person = 1.0;
        }
    }
    if (layer.border > 0.0) {
        let inner = clamp(0.5 - (d + layer.border), 0.0, 1.0);
        color = mix(layer.border_color.rgb, color, inner);
    }
    let a = clamp(0.5 - d, 0.0, 1.0) * person * texel.a;
    var out = vec4<f32>(color * a, a);

    if (layer.shadow > 0.0) {
        let b = blur();
        let ds = sd_round_rect(in.px, center + vec2<f32>(0.0, b * 0.4), half, r);
        let s = layer.shadow * (1.0 - smoothstep(-b, b, ds));
        out = out + vec4<f32>(0.0, 0.0, 0.0, s) * (1.0 - out.a);
    }
    return out * layer.opacity;
}
