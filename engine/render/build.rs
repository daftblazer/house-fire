//! Compiles the GLSL shaders to SPIR-V with glslc (shaderc; `GLSLC` names another compiler).

use std::path::PathBuf;
use std::process::Command;

/// (source, output name, defines): the vertex shader is built twice, for rigid meshes and for
/// the skinned ones (whose vertices the compute pass has blended).
const SHADERS: [(&str, &str, &[&str]); 6] = [
    ("model.vert", "model.vert", &[]),
    ("model.vert", "model_skinned.vert", &["-DSKINNED"]),
    ("model.frag", "model.frag", &[]),
    ("skin.comp", "skin.comp", &[]),
    ("overlay.vert", "overlay.vert", &[]),
    ("overlay.frag", "overlay.frag", &[]),
];

fn main() {
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    for (name, output, defines) in SHADERS {
        let src = PathBuf::from("shaders").join(name);
        println!("cargo:rerun-if-changed={}", src.display());
        let status = Command::new(std::env::var("GLSLC").unwrap_or_else(|_| "glslc".into()))
            .arg("-O")
            .args(defines)
            .arg(&src)
            .arg("-o")
            .arg(out.join(format!("{output}.spv")))
            .status()
            .expect("glslc not found: install shaderc (or set GLSLC)");
        assert!(status.success(), "glslc failed on {}", src.display());
    }
}
