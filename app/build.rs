//! Embeds the Windows application manifest (per-monitor DPI awareness v2, modern common
//! controls, asInvoker) and the DeskCorgi logo as the exe's icon when building with the
//! MSVC linker.
//!
//! The icon is turned into a compiled resource (`.res`) right here, without `rc.exe` or any
//! extra crate: a `.res` file is just a list of resources, and an `.ico` file already holds
//! the images in the form `RT_ICON` resources need. The group resource gets id 1, so it is
//! the icon Explorer shows and the one `LoadImageW(hinstance, 1, …)` loads at run time.

use std::path::Path;

/// Resource types and the flags `rc.exe` uses for them.
const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;
const FLAGS_ICON: u16 = 0x1010; // MOVEABLE | DISCARDABLE
const FLAGS_GROUP: u16 = 0x1030; // MOVEABLE | PURE | DISCARDABLE
const LANG_EN_US: u16 = 0x0409;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=windows/app.manifest");
    println!("cargo:rerun-if-changed=windows/deskcorgi.ico");
    let target = std::env::var("TARGET").unwrap_or_default();
    if target.contains("windows") && target.contains("msvc") {
        let dir = Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("windows");
        let manifest = dir.join("app.manifest");
        println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}", manifest.display());

        let ico = std::fs::read(dir.join("deskcorgi.ico")).expect("app/windows/deskcorgi.ico");
        let res = icon_res(&ico).expect("app/windows/deskcorgi.ico is not a valid .ico file");
        let out = Path::new(&std::env::var("OUT_DIR").unwrap()).join("deskcorgi_icon.res");
        std::fs::write(&out, res).unwrap();
        // link.exe accepts compiled resource files as ordinary inputs.
        println!("cargo:rustc-link-arg-bins={}", out.display());
    }
}

fn u16_at(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*b.get(i)?, *b.get(i + 1)?]))
}

fn u32_at(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes([*b.get(i)?, *b.get(i + 1)?, *b.get(i + 2)?, *b.get(i + 3)?]))
}

/// One resource entry: header with ordinal type and name, then the data padded to 4 bytes.
fn push_resource(out: &mut Vec<u8>, kind: u16, id: u16, flags: u16, data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_le_bytes()); // DataSize
    out.extend_from_slice(&32u32.to_le_bytes()); // HeaderSize
    out.extend_from_slice(&[0xFF, 0xFF]);
    out.extend_from_slice(&kind.to_le_bytes()); // TYPE (ordinal)
    out.extend_from_slice(&[0xFF, 0xFF]);
    out.extend_from_slice(&id.to_le_bytes()); // NAME (ordinal)
    out.extend_from_slice(&0u32.to_le_bytes()); // DataVersion
    out.extend_from_slice(&flags.to_le_bytes()); // MemoryFlags
    out.extend_from_slice(&LANG_EN_US.to_le_bytes()); // LanguageId
    out.extend_from_slice(&0u32.to_le_bytes()); // Version
    out.extend_from_slice(&0u32.to_le_bytes()); // Characteristics
    out.extend_from_slice(data);
    while out.len() % 4 != 0 {
        out.push(0);
    }
}

/// Builds a `.res` holding every image of `ico` as an `RT_ICON` (ids 1..n) and one
/// `RT_GROUP_ICON` (id 1) that lists them.
fn icon_res(ico: &[u8]) -> Option<Vec<u8>> {
    if u16_at(ico, 0)? != 0 || u16_at(ico, 2)? != 1 {
        return None;
    }
    let count = u16_at(ico, 4)? as usize;
    if count == 0 {
        return None;
    }
    // A .res file starts with an empty entry.
    let mut out = Vec::new();
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&32u32.to_le_bytes());
    out.extend_from_slice(&[0xFF, 0xFF, 0, 0, 0xFF, 0xFF, 0, 0]);
    out.extend_from_slice(&[0u8; 16]);

    let mut group = Vec::new();
    group.extend_from_slice(&0u16.to_le_bytes()); // reserved
    group.extend_from_slice(&1u16.to_le_bytes()); // type: icon
    group.extend_from_slice(&(count as u16).to_le_bytes());
    for n in 0..count {
        let e = 6 + n * 16;
        let entry = ico.get(e..e + 16)?;
        let size = u32_at(ico, e + 8)? as usize;
        let offset = u32_at(ico, e + 12)? as usize;
        let image = ico.get(offset..offset.checked_add(size)?)?;
        let id = (n + 1) as u16;
        push_resource(&mut out, RT_ICON, id, FLAGS_ICON, image);
        // GRPICONDIRENTRY: the ICONDIRENTRY's first 12 bytes, then the resource id.
        group.extend_from_slice(&entry[..12]);
        group.extend_from_slice(&id.to_le_bytes());
    }
    push_resource(&mut out, RT_GROUP_ICON, 1, FLAGS_GROUP, &group);
    Some(out)
}
