#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("GTK parent native probe requires Linux");
    std::process::exit(2);
}

#[cfg(target_os = "linux")]
fn main() {
    println!("{}", grok_computer_use_gtk_parent::probe::run());
}
