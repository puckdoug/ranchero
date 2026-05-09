// SPDX-License-Identifier: AGPL-3.0-only

fn main() {
    let proto_root = "proto";
    let files = [
        "proto/login.proto",
        "proto/per-session-info.proto",
        "proto/profile.proto",
        "proto/udp-node-msgs.proto",
        "proto/tcp-node-msgs.proto",
        "proto/events.proto",
        "proto/segment-result.proto",
    ];
    for f in &files {
        println!("cargo:rerun-if-changed={f}");
    }
    prost_reflect_build::Builder::new()
        .file_descriptor_set_bytes("crate::FILE_DESCRIPTOR_SET_BYTES")
        .compile_protos(&files, &[proto_root])
        .expect("prost-reflect-build: compile_protos");
}
