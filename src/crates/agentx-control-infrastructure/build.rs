fn main() {
    println!("cargo:rerun-if-changed=../../../deploy/migrations/control");
}
