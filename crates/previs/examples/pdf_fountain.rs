//! Print the Fountain a PDF reads as — the `pdf` module's view of a file,
//! for checking a new generator's output by eye:
//! `cargo run -p localgpt-previs --example pdf_fountain -- script.pdf`.
fn main() {
    let path = std::env::args().nth(1).expect("a PDF path");
    let bytes = std::fs::read(&path).expect("read the PDF");
    match localgpt_previs::pdf::to_fountain(&bytes, None) {
        Ok(text) => print!("{text}"),
        Err(e) => {
            eprintln!("{path}: {e}");
            std::process::exit(1);
        }
    }
}
