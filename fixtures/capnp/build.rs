fn main() {
    println!("cargo:rerun-if-env-changed=RYDANTIC_REGENERATE_CAPNP");
    println!("cargo:rerun-if-changed=schema/wire.capnp");
    // Committed official bindings let consumers and CI build without the system compiler.
    if std::env::var_os("RYDANTIC_REGENERATE_CAPNP").is_some() {
        capnpc::CompilerCommand::new()
            .src_prefix("schema")
            .file("schema/wire.capnp")
            .output_path("src")
            .run()
            .expect("generate native Cap'n Proto bindings");
    }
}
