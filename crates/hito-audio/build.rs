#[cfg(feature = "device")]
fn main() {
    println!("cargo:rerun-if-changed=native");
    let mut build = cc::Build::new();
    build
        .cpp(true)
        .std("c++11")
        .file("native/tempo.cpp")
        .include("native/soundtouch-2.3.2");
    if cfg!(feature = "native-probe") {
        build.define("HITO_NATIVE_PROBE", None);
    }
    build.compile("hito_tempo");
}

#[cfg(not(feature = "device"))]
fn main() {}
