fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proto_root = "proto/sui-apis/proto";

    // Compile google.rpc protos first into their own file
    prost_build::Config::new()
        .out_dir(format!("{}/", std::env::var("OUT_DIR")?))
        .compile_protos(
            &[format!("{proto_root}/google/rpc/status.proto")],
            &[proto_root],
        )?;

    // Compile Sui protos, mapping google.rpc to our module
    tonic_build::configure()
        .build_server(false)
        .build_client(true)
        .extern_path(".google.rpc", "crate::google_rpc")
        .compile_protos(
            &[
                format!("{proto_root}/sui/rpc/v2/subscription_service.proto"),
                format!("{proto_root}/sui/rpc/v2/ledger_service.proto"),
                format!("{proto_root}/sui/rpc/v2/transaction_execution_service.proto"),
                format!("{proto_root}/sui/rpc/v2/state_service.proto"),
            ],
            &[proto_root],
        )?;

    Ok(())
}
