use std::env;

fn main() {
    println!("cargo:rustc-check-cfg=cfg(quickjs_ng_native)");
    println!("cargo:rustc-check-cfg=cfg(quickjs_ng_browser_c_vm_sys)");

    let quickjs_feature_enabled = env::var_os("CARGO_FEATURE_QUICKJS_NG_ENGINE").is_some();
    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let browser_wasm = target_arch == "wasm32" && target_os == "unknown";

    if quickjs_feature_enabled {
        println!("cargo:rustc-cfg=quickjs_ng_native");
    }
    if quickjs_feature_enabled && browser_wasm {
        println!("cargo:rustc-cfg=quickjs_ng_browser_c_vm_sys");
    }
}
