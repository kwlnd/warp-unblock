// Иконка exe (Проводник, панель задач, ярлыки). Окно и трей берут PNG из assets/icons.
fn main() {
    println!("cargo:rerun-if-changed=assets/icons/app.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icons/app.ico");
        res.set("ProductName", "WARP Unblock");
        res.set("FileDescription", "WARP Unblock");
        res.compile().expect("compile Windows resources");
    }
}
