// Põe o ícone (assets/seek.ico) dentro do Seek.exe.
fn main() {
    println!("cargo:rerun-if-changed=assets/seek.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/seek.ico");
        if let Err(e) = res.compile() {
            println!("cargo:warning=não foi possível embutir o ícone: {e}");
        }
    }
}
