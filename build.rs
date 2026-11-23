#[allow(dead_code)]
mod icon_art {
    include!("src/icon_art.rs");
}

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=src/icon_art.rs");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/app.manifest");
    println!("cargo:rerun-if-changed=src/native/kage_player.c");
    let media = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("third_party").join("install");
    cc::Build::new().file("src/native/kage_player.c").include(media.join("include")).opt_level(2).warnings(false).compile("kage_player");
    println!("cargo:rustc-link-search=native={}", media.join("lib").display());
    for lib in ["avformat", "avcodec", "swscale", "swresample", "avutil", "dav1d", "atomic"] {
        println!("cargo:rustc-link-lib=static={lib}");
    }
    for lib in ["bcrypt", "ole32", "user32"] {
        println!("cargo:rustc-link-lib=dylib={lib}");
    }
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let ico_path = out_dir.join("kage.ico");
    std::fs::write(&ico_path, icon_art::build_ico(&[16, 20, 24, 32, 40, 48, 64, 128, 256])).unwrap();
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("assets").join("app.manifest");
    let rc_path = out_dir.join("kage.rc");
    let version = std::env::var("CARGO_PKG_VERSION").unwrap();
    let mut parts: Vec<u32> = version.split(['.', '-']).filter_map(|p| p.parse().ok()).collect();
    parts.resize(4, 0);
    let numeric = parts.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(",");
    let version_info = format!(
        "1 VERSIONINFO\nFILEVERSION {numeric}\nPRODUCTVERSION {numeric}\nFILEOS 0x40004\nFILETYPE 0x1\nBEGIN\n BLOCK \"StringFileInfo\"\n BEGIN\n  BLOCK \"040904B0\"\n  BEGIN\n   VALUE \"FileDescription\", \"Kage Replay\"\n   VALUE \"ProductName\", \"Kage Replay\"\n   VALUE \"FileVersion\", \"{version}\"\n   VALUE \"ProductVersion\", \"{version}\"\n   VALUE \"InternalName\", \"KageReplay\"\n   VALUE \"OriginalFilename\", \"KageReplay.exe\"\n  END\n END\n BLOCK \"VarFileInfo\"\n BEGIN\n  VALUE \"Translation\", 0x409, 1200\n END\nEND\n"
    );
    let rc = format!(
        "1 ICON \"{}\"\n1 24 \"{}\"\n{version_info}",
        ico_path.display().to_string().replace('\\', "\\\\"),
        manifest.display().to_string().replace('\\', "\\\\")
    );
    std::fs::write(&rc_path, rc).unwrap();
    embed_resource::compile(&rc_path, embed_resource::NONE).manifest_required().unwrap();
}
