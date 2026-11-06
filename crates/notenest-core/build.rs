fn main() {
    // Re-embed migrations when they change.
    println!("cargo:rerun-if-changed=migrations");
}
