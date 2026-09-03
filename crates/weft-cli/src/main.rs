use std::ffi::OsString;
use std::io;

fn main() {
    let code = weft_cli::run(
        std::env::args_os().skip(1).collect::<Vec<OsString>>(),
        &mut io::stdout().lock(),
        &mut io::stderr().lock(),
    );
    std::process::exit(code);
}
