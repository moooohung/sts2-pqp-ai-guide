fn main() {
    println!("cargo:rerun-if-env-changed=STS2_ADVISOR_SOURCE_COMMIT");
}
