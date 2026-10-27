//! Builds `assets/chalkberry.rc` into the executable, so the Chalkberry
//! icon shows in Explorer, the taskbar, shortcuts and the tray.

fn main() {
    println!("cargo:rerun-if-changed=assets/chalkberry.rc");
    println!("cargo:rerun-if-changed=assets/chalkberry.ico");
    // The include dir lets the resource compiler find the .ico next to
    // the .rc, whichever folder it starts from.
    let include_dirs = embed_resource::ParamsIncludeDirs(&["assets"]);
    embed_resource::compile("assets/chalkberry.rc", include_dirs)
        .manifest_optional()
        .expect("could not build the app icon into the executable");
}
