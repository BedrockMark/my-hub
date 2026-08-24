fn main() {
    // Compile Slint UI files
    slint_build::compile("ui/app.slint").expect("Failed to compile Slint UI files");

    // Re-run if default manifest or assets change
    println!("cargo:rerun-if-changed=ui/");
    println!("cargo:rerun-if-changed=assets/default-manifest.toml");
}