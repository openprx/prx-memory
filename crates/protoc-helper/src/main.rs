fn main() {
    match protoc_bin_vendored::protoc_bin_path() {
        Ok(path) => println!("{}", path.display()),
        Err(e) => {
            eprintln!("failed to locate vendored protoc binary: {e}");
            std::process::exit(1);
        }
    }
}
