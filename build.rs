fn main() {
    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("FileDescription", "SpotLight");
        res.set("ProductName", "SpotLight");
        res.set("ProductVersion", "1.0.0");
        res.set("FileVersion", "1.0.0");
        res.set("InternalName", "SpotLight");
        res.set("OriginalFilename", "spotlight.exe");
        res.set("CompanyName", "SpotLight");
        if let Err(e) = res.compile() {
            eprintln!("Warning: Failed to compile windows resource: {}", e);
        }
    }
}
