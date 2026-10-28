fn main() {
    println!(
        "cargo:rustc-env=WAKEIT_BUILD_TARGET={}",
        std::env::var("TARGET").unwrap()
    );
}
