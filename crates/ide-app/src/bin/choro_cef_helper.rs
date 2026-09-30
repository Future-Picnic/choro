#[cfg(target_os = "macos")]
fn main() {
    use cef::{args::Args, *};

    let args = Args::new();
    let mut sandbox = cef::sandbox::Sandbox::new();
    sandbox.initialize(args.as_main_args());

    let loader = cef::library_loader::LibraryLoader::new(
        &std::env::current_exe().expect("CEF helper executable path"),
        true,
    );
    assert!(loader.load(), "could not load bundled Chromium framework");
    assert!(
        !cef::api_hash(cef::sys::CEF_API_VERSION_LAST, 0).is_null(),
        "Chromium framework and Rust bindings do not match"
    );

    let exit_code = cef::execute_process(
        Some(args.as_main_args()),
        None::<&mut App>,
        std::ptr::null_mut(),
    );
    if exit_code >= 0 {
        std::process::exit(exit_code);
    }
    eprintln!("choro-cef-helper was launched without a Chromium process type");
    std::process::exit(1);
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("Choro's Chromium helper is only used on macOS");
}
