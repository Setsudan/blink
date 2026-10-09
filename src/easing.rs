/// Hermite smoothstep: maps `t` in \[0, 1\] to an eased curve.
pub fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
