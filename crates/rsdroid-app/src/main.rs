//! rsdroid manager UI.
//!
//! M0 only reserves the binary. The egui manager and per-instance windows
//! arrive in M3; until then `rsdroid doctor` is the useful entry point.

fn main() {
    println!(
        "rsdroid {} manager UI is not implemented yet; it arrives in M3.",
        rsdroid_core::VERSION
    );
    println!("Run `rsdroid doctor` to check this machine.");
}
