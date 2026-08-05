fn main() -> Result<(), Box<dyn std::error::Error>> {
    let protoc = protoc_bin_vendored::protoc_bin_path()?;
    unsafe {
        std::env::set_var("PROTOC", protoc);
    }
    tonic_build::configure()
        .compile_protos(&["proto/runtime.proto", "proto/sandbox.proto"], &["proto"])?;
    println!("cargo:rerun-if-changed=proto/runtime.proto");
    println!("cargo:rerun-if-changed=proto/sandbox.proto");
    Ok(())
}
