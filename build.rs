//! Build data/resources (Jelly's own icons) into the app, with GLib's
//! glib-compile-resources. And on Windows, give jelly.exe its icon and the
//! name and version Explorer and Task Manager show, as a resource compiled by
//! windres (from MinGW's binutils; set WINDRES to use another).

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    compile_resources();
    windows_resource();
}

/// For `gio::resources_register_include!("jelly.gresource")`
fn compile_resources() {
    println!("cargo::rerun-if-changed=data/resources");
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let status = Command::new("glib-compile-resources")
        .args(["--sourcedir", "data/resources", "--target"])
        .arg(out.join("jelly.gresource"))
        .arg("data/resources/resources.gresource.xml")
        .status()
        .unwrap_or_else(|e| panic!("couldn't run glib-compile-resources: {e}"));
    assert!(status.success(), "glib-compile-resources failed");
}

fn windows_resource() {
    println!("cargo::rerun-if-changed=data/windows/jelly.ico");
    println!("cargo::rerun-if-env-changed=WINDRES");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let version = env::var("CARGO_PKG_VERSION").unwrap();
    let number = |part: &str| env::var(format!("CARGO_PKG_VERSION_{part}")).unwrap();
    let numbers = format!(
        "{},{},{},0",
        number("MAJOR"),
        number("MINOR"),
        number("PATCH")
    );
    let icon =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("data/windows/jelly.ico");
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let script = format!(
        r#"1 ICON "{icon}"
1 VERSIONINFO
FILEVERSION {numbers}
PRODUCTVERSION {numbers}
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "FileDescription", "Jelly"
      VALUE "ProductName", "Jelly"
      VALUE "FileVersion", "{version}"
      VALUE "ProductVersion", "{version}"
      VALUE "OriginalFilename", "jelly.exe"
      VALUE "LegalCopyright", "GPL-3.0-or-later"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
        icon = icon.display().to_string().replace('\\', "/"),
    );
    std::fs::write(out.join("jelly.rc"), script).unwrap();
    let object = out.join("jelly-resources.o");
    let windres = env::var("WINDRES").unwrap_or_else(|_| "windres".into());
    let status = Command::new(&windres)
        .arg(out.join("jelly.rc"))
        .args(["--output-format", "coff", "--output"])
        .arg(&object)
        .status()
        .unwrap_or_else(|e| panic!("couldn't run {windres}: {e}"));
    assert!(status.success(), "{windres} failed");
    println!("cargo::rustc-link-arg-bins={}", object.display());
}
