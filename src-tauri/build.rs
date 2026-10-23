fn main() {
    #[cfg(target_os = "macos")]
    {
        cc::Build::new()
            .file("native/HWNativeShell.m")
            .file("native/HWUpdatePanel.m")
            .file("native/HWMobileBridge.m")
            .flag("-fobjc-arc")
            .compile("harmony_native_shell");
        println!("cargo:rustc-link-lib=framework=AppKit");
        println!("cargo:rustc-link-lib=framework=WebKit");
        println!("cargo:rustc-link-lib=framework=CoreImage");
        println!("cargo:rerun-if-changed=native/HWNativeShell.m");
        println!("cargo:rerun-if-changed=native/HWUpdatePanel.m");
        println!("cargo:rerun-if-changed=native/HWMobileBridge.m");
    }

    #[cfg(target_os = "windows")]
    {
        let mut attributes = tauri_build::Attributes::new();
        attributes = attributes
            .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());
        add_windows_manifest();
        tauri_build::try_build(attributes).expect("failed to run tauri build");
    }

    #[cfg(not(target_os = "windows"))]
    tauri_build::build()
}

#[cfg(target_os = "windows")]
fn add_windows_manifest() {
    let manifest = std::env::current_dir()
        .expect("build.rs cwd")
        .join("windows-app-manifest.xml");

    println!("cargo:rerun-if-changed={}", manifest.display());
    println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    println!("cargo:rustc-link-arg=/WX");
}
