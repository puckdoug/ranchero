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
    prost_build::Config::new()
        .compile_protos(&files, &[proto_root])
        .expect("prost-build: compile_protos");
}
