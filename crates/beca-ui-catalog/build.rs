fn main() {
    slint_build::compile("ui/catalog.slint").expect("Slint build failed");
    println!("cargo:rerun-if-changed=ui");
    println!("cargo:rerun-if-changed=../beca-ui/ui");
}
