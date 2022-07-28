#[cfg(target_os = "windows")]
use winres::WindowsResource;

fn main() {
    qt_ritual_build::add_resources(concat!(env!("CARGO_MANIFEST_DIR"), "/ui/resources.qrc"));
    //println!("cargo:rustc-link-lib=static=rqwebview");
    //println!("cargo:rustc-link-lib=Qt5WebKitWidgets");
    //println!("cargo:rustc-link-search=./rqwebview");
    #[cfg(target_os = "windows")]
    {
        WindowsResource::new()
            .set_icon("ui/i/ecmui.ico")
            .compile()
            .unwrap();
    }
}
