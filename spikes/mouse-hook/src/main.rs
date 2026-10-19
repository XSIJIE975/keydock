mod geometry;
#[cfg(windows)]
mod imp;

#[cfg(windows)]
fn main() {
    std::process::exit(imp::run());
}

#[cfg(not(windows))]
fn main() {
    eprintln!("keydock-mouse-spike currently has a Windows implementation only");
    std::process::exit(2);
}
