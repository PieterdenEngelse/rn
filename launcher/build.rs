//! Version information for the Windows launcher.
//!
//! A Windows executable carries its product name and version in a VERSIONINFO
//! resource: what Explorer's Details tab shows, and what SignPath checks before
//! it will sign anything. Its terms require every signed binary's metadata to
//! be set and enforced, with the product name matching the project, so an
//! rn.exe without this resource cannot be released signed (docs/signing.md).
//!
//! No crate for it, on purpose. The launcher is dependency-light because it has
//! to run before anything else can, and a resource is a few dozen lines of
//! text: this writes the .rc with the version already filled in, compiles it
//! with the resource compiler that is on hand, and hands the .res straight to
//! the linker, which lld-link and link.exe both accept as an input.
//!
//! Which compiler: llvm-rc for the cross-build on Linux (package.sh --target
//! windows, and the release workflow), rc.exe from the Windows SDK for a native
//! build. RN_RC names one explicitly. When neither is found the build goes on
//! without the resource and says so: a development build has no use for it,
//! and package.sh is what refuses to ship an rn.exe that lacks it.
//!
//! Does nothing at all for any other target.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=RN_RC");

    let os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if os != "windows" || target_env != "msvc" {
        return;
    }

    let out = PathBuf::from(env::var("OUT_DIR").expect("cargo sets OUT_DIR"));
    let version = env::var("CARGO_PKG_VERSION").expect("cargo sets CARGO_PKG_VERSION");
    let rc = out.join("rn.rc");
    let res = out.join("rn.res");
    fs::write(&rc, resource_script(&version)).expect("write rn.rc");

    match compile(&rc, &res) {
        Ok(_) => {
            // -bins: the resource belongs to rn.exe, not to the library's tests.
            println!("cargo:rustc-link-arg-bins={}", res.display());
        }
        Err(why) => println!(
            "cargo:warning=rn.exe built WITHOUT version information ({why}). \
             Fine for development; package.sh refuses to ship it. \
             Install llvm (llvm-rc) or the Windows SDK (rc.exe), or set RN_RC."
        ),
    }
}

/// The resource, with every value written in. No #define or #include, so it
/// needs no preprocessor, which llvm-rc would otherwise go looking for clang
/// to run.
fn resource_script(version: &str) -> String {
    let mut nums = version
        .split(['.', '-', '+'])
        .map(|n| n.parse::<u16>().unwrap_or(0));
    let major = nums.next().unwrap_or(0);
    let minor = nums.next().unwrap_or(0);
    let patch = nums.next().unwrap_or(0);
    format!(
        r#"1 VERSIONINFO
FILEVERSION {major},{minor},{patch},0
PRODUCTVERSION {major},{minor},{patch},0
FILEFLAGSMASK 0x3F
FILEFLAGS 0x0
FILEOS 0x40004
FILETYPE 0x1
FILESUBTYPE 0x0
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "Pieter den Engelse"
      VALUE "FileDescription", "rn launcher"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "rn"
      VALUE "LegalCopyright", "Copyright (c) Pieter den Engelse. MIT OR Apache-2.0."
      VALUE "OriginalFilename", "rn.exe"
      VALUE "ProductName", "rn"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#
    )
}

/// Runs the first resource compiler that works, and names it.
fn compile(rc: &Path, res: &Path) -> Result<String, String> {
    let explicit = env::var("RN_RC").ok().filter(|s| !s.is_empty());
    let candidates: Vec<String> = match explicit {
        Some(tool) => vec![tool],
        None => vec!["llvm-rc".into(), "rc.exe".into(), "rc".into()],
    };
    let mut tried = Vec::new();
    for tool in candidates {
        // llvm-rc takes -no-preprocess; rc.exe does not know it and needs none.
        let llvm = tool.contains("llvm-rc");
        let mut args: Vec<String> = Vec::new();
        if llvm {
            args.push("-no-preprocess".into());
        }
        args.push("/fo".into());
        args.push(res.display().to_string());
        args.push(rc.display().to_string());

        // A build script runs the resource compiler, not Node: the
        // NodeCommand rule in clippy.toml is about the Node child's
        // environment, which nothing here touches.
        #[allow(clippy::disallowed_methods)]
        let status = std::process::Command::new(&tool).args(&args).status();
        match status {
            Ok(s) if s.success() && res.exists() => return Ok(tool),
            Ok(s) => tried.push(format!("{tool} exited {s}")),
            Err(_) => tried.push(format!("{tool} not found")),
        }
    }
    Err(tried.join("; "))
}
