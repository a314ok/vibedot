//! Writes the bundled example projects: `cargo run -p vd_engine --example gen_templates -- <dir>`.
fn main() -> std::io::Result<()> {
    let out = std::env::args().nth(1).unwrap_or_else(|| "examples".into());
    let out = std::path::Path::new(&out);
    vd_engine::templates::write_to(&out.join("neon_drift"), &vd_engine::templates::neon_drift())?;
    vd_engine::templates::write_to(&out.join("hello"), &vd_engine::templates::empty_project("Hello Vibedot"))?;
    println!("wrote examples to {}", out.display());
    Ok(())
}
