struct ViewParams {
    center: vec2f,
    range: vec2f,
    screen_dims: vec2u,
};

@group(0) @binding(0) var<uniform> params: ViewParams;
@group(0) @binding(1) var output_texture: texture_storage_2d<rgba8unorm, write>;

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> vec4f {
    if s == 0.0 { return vec4f(v, v, v, 1.0); }
    let h_sector = h / 60.0;
    let i = i32(floor(h_sector));
    let f = h_sector - f32(i);
    let p = v * (1.0 - s);
    let q = v * (1.0 - s * f);
    let t = v * (1.0 - s * (1.0 - f));

    var rgb: vec3f;
    switch (i) {
        case 0: { rgb = vec3f(v, t, p); }
        case 1: { rgb = vec3f(q, v, p); }
        case 2: { rgb = vec3f(p, v, t); }
        case 3: { rgb = vec3f(p, q, v); }
        case 4: { rgb = vec3f(t, p, v); }
        default: { rgb = vec3f(v, p, q); }
    }
    return vec4f(rgb, 1.0);
}

fn map_pixel_to_point(pixel: vec2u) -> vec2f {
    let norm = vec2f(f32(pixel.x), f32(pixel.y)) / vec2f(f32(params.screen_dims.x), f32(params.screen_dims.y));
    let norm_centered = norm - 0.5;
    return params.center + (norm_centered * params.range);
}

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let pixel = global_id.xy;
    if (pixel.x >= params.screen_dims.x || pixel.y >= params.screen_dims.y) {
        return;
    }

    let max_iterations = 1000u;
    var iterations = 0u;

    let c = map_pixel_to_point(pixel);
    var z = vec2f(0.0, 0.0);

    // TODO: Implement the Mandelbrot iteration loop
    // The formula is: z_{n+1} = z_n^2 + c
    // Loop while |z|^2 <= 4.0 and iterations < max_iterations

    // TODO: Implement the while loop
    while (iterations < max_iterations && (z.x * z.x + z.y * z.y) <= 4.0) {
        let z_real_new = z.x * z.x - z.y * z.y + c.x;
        let z_imag_new = 2.0 * z.x * z.y + c.y;
        z = vec2f(z_real_new, z_imag_new);
        iterations = iterations + 1u;
    }

    var color: vec4f;
    if iterations == max_iterations {
        // Point is in the Mandelbrot set - use angle-based coloring
        // TODO: Calculate the angle and hue
        // let angle = 0.0; // Replace with atan2(z.y, z.x)
        let angle = atan2(z.y, z.x);
        let hue_norm = (angle + 3.1415926535) / (2.0 * 3.1415926535);
        let hue = hue_norm * 360.0;
        color = hsv_to_rgb(hue, 1.0, 1.0);
    } else {
        // Point escaped -> color based on iteration count
        // TODO: Calculate hue based on iteration count
        // let hue = 0.0; // Replace with (f32(iterations) / f32(max_iterations)) * 360.0
        let hue = (f32(iterations) / f32(max_iterations)) * 360.0;
        color = hsv_to_rgb(hue, 1.0, 1.0);
    }

    textureStore(output_texture, pixel, color);
}