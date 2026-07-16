fn main() {
    // protoc is not installed in this environment; vendor it via
    // protoc-bin-vendored so tonic-build can compile plugin.proto without a
    // system dependency (protobuf-src was tried first but requires cmake,
    // which is also unavailable here — see docs/decisions/0002-protoc-vendoring.md).
    std::env::set_var(
        "PROTOC",
        protoc_bin_vendored::protoc_bin_path().expect("failed to locate vendored protoc"),
    );

    tonic_build::configure()
        .build_server(true)
        .build_client(false)
        .compile_protos(&["proto/plugin.proto"], &["proto"])
        .expect("failed to compile plugin.proto");

    println!("cargo:rerun-if-changed=proto/plugin.proto");
}
