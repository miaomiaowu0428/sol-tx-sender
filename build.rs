fn main() -> Result<(), Box<dyn std::error::Error>> {
    let protoc = protoc_bin_vendored::protoc_bin_path()?;
    // edition 2024 中 set_var 是 unsafe
    unsafe { std::env::set_var("PROTOC", protoc); }

    tonic_build::configure()
        .build_server(false)
        .compile_protos(
            &[
                "proto/auth.proto",
                "proto/searcher.proto",
                "proto/bundle.proto",
                "proto/packet.proto",
                "proto/shared.proto",
            ],
            &["proto"],
        )?;

    Ok(())
}
