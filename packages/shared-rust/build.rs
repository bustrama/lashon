//! Build script: generate the STT gRPC client from packages/proto/stt.proto.

fn main() {
    // Use a vendored protoc so neither developers nor CI need a system
    // protobuf-compiler installed. Safe on edition 2021 (set_var is not unsafe).
    let protoc = protoc_bin_vendored::protoc_bin_path().expect("vendored protoc binary");
    std::env::set_var("PROTOC", protoc);

    // Lashon is the gRPC client; the Python sidecar is the server.
    tonic_build::configure()
        .build_server(false)
        .compile_protos(&["../proto/stt.proto"], &["../proto"])
        .expect("failed to compile packages/proto/stt.proto");

    println!("cargo:rerun-if-changed=../proto/stt.proto");
}
