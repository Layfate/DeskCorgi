//! Writes the built-in skins to `skins/*.toml` as templates for custom skins.
//! Run with `cargo run -p deskcorgi-core --example export_skins`.

use std::path::Path;

fn main() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skins");
    std::fs::create_dir_all(&dir).expect("create skins dir");
    for (id, make) in deskcorgi_core::skin::BUILTINS {
        let file = format!("{id}.toml");
        std::fs::write(dir.join(&file), make().to_toml()).expect("write skin");
        println!("wrote skins/{file}");
    }
}
