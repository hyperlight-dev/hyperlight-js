fn main() {
    if std::env::var_os("CARGO_CFG_HYPERLIGHT").is_none() {
        return;
    }

    let files = ["stubs/clock.c", "stubs/localtime.c"];

    for file in files {
        println!("cargo:rerun-if-changed={}", file);
    }

    cc::Build::new().files(files).compile("stubs");
}
