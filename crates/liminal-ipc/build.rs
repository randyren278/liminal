fn main() {
    // Vendors and builds `protoc` from source so this crate has no dependency on a
    // system-installed Protocol Buffers compiler (CI runs a clean checkout).
    std::env::set_var("PROTOC", protobuf_src::protoc());

    prost_build::compile_protos(&["../../proto/liminal.proto"], &["../../proto"])
        .expect("failed to compile proto/liminal.proto");
}
