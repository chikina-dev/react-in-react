use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{self},
};

const WASI_SDK_VERSION_MAJOR: usize = 24;
const WASI_SDK_VERSION_MINOR: usize = 0;

const HEADER_FILES: &[&str] = &[
    "builtin-array-fromasync.h",
    "dtoa.h",
    "libregexp-opcode.h",
    "libregexp.h",
    "libunicode-table.h",
    "libunicode.h",
    "list.h",
    "quickjs-atom.h",
    "quickjs-opcode.h",
    "quickjs-c-atomics.h",
    "quickjs.h",
    "cutils.h",
];

const SOURCE_FILES: &[&str] = &[
    "libregexp.c",
    "libunicode.c",
    "cutils.c",
    "quickjs.c",
    "dtoa.c",
];

fn vendor_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../vendor/rquickjs-sys")
        .canonicalize()
        .expect("vendor/rquickjs-sys must exist")
}

fn quickjs_src_dir() -> PathBuf {
    vendor_dir().join("quickjs")
}

fn quickjs_bind_header() -> PathBuf {
    vendor_dir().join("quickjs.bind.h")
}

fn download_wasi_sdk() -> PathBuf {
    let mut wasi_sdk_dir: PathBuf = env::var("OUT_DIR").unwrap().into();
    wasi_sdk_dir.push("wasi-sdk");

    fs::create_dir_all(&wasi_sdk_dir).unwrap();

    let mut archive_path = wasi_sdk_dir.clone();
    archive_path.push(format!(
        "wasi-sdk-{}-{}.tar.gz",
        WASI_SDK_VERSION_MAJOR, WASI_SDK_VERSION_MINOR
    ));

    if !archive_path.try_exists().unwrap() {
        let file_suffix = match (env::consts::OS, env::consts::ARCH) {
            ("linux", "x86") | ("linux", "x86_64") => "x86_64-linux",
            ("linux", "aarch64") => "arm64-linux",
            ("macos", "x86") | ("macos", "x86_64") => "x86_64-macos",
            ("macos", "aarch64") => "arm64-macos",
            ("windows", "x86") | ("windows", "x86_64") => "x86_64-windows",
            ("windows", "aarch64") => "arm64-windows",
            other => panic!("Unsupported platform tuple {:?}", other),
        };

        let uri = format!(
            "https://github.com/WebAssembly/wasi-sdk/releases/download/wasi-sdk-{}/wasi-sdk-{}.{}-{}.tar.gz",
            WASI_SDK_VERSION_MAJOR,
            WASI_SDK_VERSION_MAJOR,
            WASI_SDK_VERSION_MINOR,
            file_suffix
        );

        let output = process::Command::new("curl")
            .args([
                "--location",
                "-o",
                archive_path.to_string_lossy().as_ref(),
                uri.as_ref(),
            ])
            .output()
            .expect("failed to download the WASI SDK with curl");
        if !output.status.success() {
            panic!(
                "curl WASI SDK failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    let mut test_binary = wasi_sdk_dir.clone();
    test_binary.extend(["bin", "wasm-ld"]);
    if !test_binary.try_exists().unwrap() {
        let output = process::Command::new("tar")
            .args([
                "-zxf",
                archive_path.to_string_lossy().as_ref(),
                "--strip-components",
                "1",
            ])
            .current_dir(&wasi_sdk_dir)
            .output()
            .unwrap();
        if !output.status.success() {
            panic!(
                "Unpacking WASI SDK failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    wasi_sdk_dir
}

fn get_wasi_sdk_path() -> PathBuf {
    std::env::var_os("WASI_SDK")
        .map(PathBuf::from)
        .unwrap_or_else(download_wasi_sdk)
}

fn main() {
    println!("cargo:rerun-if-env-changed=CARGO_CFG_SANITIZE");
    println!("cargo:rerun-if-env-changed=WASI_SDK");
    println!("cargo:rerun-if-changed={}", quickjs_bind_header().display());
    println!("cargo:rerun-if-changed={}", quickjs_src_dir().display());

    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap();
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap();

    if target_arch == "wasm32" && target_os == "unknown" {
        panic!(
            "quickjs-ng-sys does not yet support browser target wasm32-unknown-unknown: \
             QuickJS-NG needs a browser-wasm specific libc/sysroot strategy instead of the \
             WASI-only headers shipped by current bindings"
        );
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("No OUT_DIR env var is set by cargo"));
    let mut defines: Vec<(String, Option<&str>)> = vec![("_GNU_SOURCE".into(), None)];

    #[cfg(feature = "disable-assertions")]
    defines.push(("NDEBUG".into(), None));

    if target_arch == "wasm32" {
        defines.push(("EMSCRIPTEN".into(), Some("1")));
        defines.push(("FE_DOWNWARD".into(), Some("0")));
        defines.push(("FE_UPWARD".into(), Some("0")));
    }

    if target_os == "windows" {
        if target_env == "msvc" {
            unsafe {
                env::set_var(
                    "CFLAGS",
                    "/DWIN32_LEAN_AND_MEAN /std:c11 /experimental:c11atomics",
                );
            }
        } else {
            unsafe {
                env::set_var("CFLAGS", "-DWIN32_LEAN_AND_MEAN -std=c11");
            }
        }
    }

    let src_dir = quickjs_src_dir();
    for file in SOURCE_FILES.iter().chain(HEADER_FILES.iter()) {
        fs::copy(src_dir.join(file), out_dir.join(file))
            .expect("Unable to copy QuickJS-NG source into OUT_DIR");
    }
    fs::copy(quickjs_bind_header(), out_dir.join("quickjs.bind.h"))
        .expect("Unable to copy quickjs.bind.h into OUT_DIR");

    let mut builder = cc::Build::new();
    builder
        .extra_warnings(false)
        .flag_if_supported("-Wno-implicit-const-int-float-conversion");

    match env::var("CARGO_CFG_SANITIZE").as_deref() {
        Ok("address") => {
            builder
                .flag("-fsanitize=address")
                .flag("-fno-sanitize-recover=all")
                .flag("-fno-omit-frame-pointer");
        }
        Ok("memory") => {
            builder
                .flag("-fsanitize=memory")
                .flag("-fno-sanitize-recover=all")
                .flag("-fno-omit-frame-pointer");
        }
        Ok("thread") => {
            builder
                .flag("-fsanitize=thread")
                .flag("-fno-sanitize-recover=all")
                .flag("-fno-omit-frame-pointer");
        }
        Ok(other) => println!("cargo:warning=Unsupported sanitize_option: '{other}'"),
        _ => {}
    }

    if target_arch == "wasm32" && target_os == "wasi" {
        let wasi_sdk_path = get_wasi_sdk_path();
        if !wasi_sdk_path.try_exists().unwrap() {
            panic!(
                "wasi-sdk not installed in specified path of {}",
                wasi_sdk_path.display()
            );
        }
        unsafe {
            env::set_var("CC", wasi_sdk_path.join("bin/clang").to_str().unwrap());
            env::set_var("AR", wasi_sdk_path.join("bin/ar").to_str().unwrap());
        }
        let include_dir = wasi_sdk_path.join("share/wasi-sysroot/include");
        let sysroot = format!(
            "--sysroot={}",
            wasi_sdk_path.join("share/wasi-sysroot").display()
        );
        let include_flag = format!("-isystem{}", include_dir.display());
        unsafe {
            env::set_var("CFLAGS", format!("{sysroot} {include_flag}"));
        }
        builder.include(include_dir);
    }

    for (name, value) in &defines {
        builder.define(name, *value);
    }

    for src in SOURCE_FILES {
        builder.file(out_dir.join(src));
    }

    builder.compile("libquickjs-ng.a");

    let target = env::var("TARGET").unwrap();
    fs::write(
        out_dir.join("bindings_target.rs"),
        format!(
            r#"macro_rules! bindings_env {{
                ("TARGET") => {{ "{target}" }};
            }}"#
        ),
    )
    .expect("Unable to write bindings_target.rs");
}
