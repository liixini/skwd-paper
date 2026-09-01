fn main() {
    println!("cargo:rustc-link-lib=yuv");
    println!("cargo:rerun-if-changed=build.rs");
}
