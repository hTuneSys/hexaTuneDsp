// SPDX-FileCopyrightText: 2026 hexaTune LLC
// SPDX-License-Identifier: MIT

fn main() {
    let crate_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();

    let config =
        cbindgen::Config::from_file(format!("{crate_dir}/cbindgen.toml")).unwrap_or_default();

    cbindgen::Builder::new()
        .with_crate(&crate_dir)
        .with_config(config)
        .generate()
        .map(|bindings| {
            bindings.write_to_file("include/hexatune_dsp_ffi.h");
        })
        .ok();
}
