fn main() {
    let path = protoc_bin_vendored::protoc_bin_path().expect("vendored protoc path");
    println!("{}", path.display());
}
