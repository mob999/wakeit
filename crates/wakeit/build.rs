fn main() {
    println!("cargo:rerun-if-changed=../../web/dist");
    println!(
        "cargo:rustc-env=WAKEIT_BUILD_TARGET={}",
        std::env::var("TARGET").unwrap()
    );
}
