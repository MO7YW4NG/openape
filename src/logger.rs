/// CLI logger with colored output and silent mode support.
pub struct Logger {
    verbose: bool,
    silent: bool,
    no_color: bool,
}

impl Logger {
    pub fn new(verbose: bool, silent: bool) -> Self {
        let no_color = std::env::var("NO_COLOR").is_ok();
        Self {
            verbose,
            silent,
            no_color,
        }
    }

    fn color(&self, code: &str, text: &str) -> String {
        if self.no_color {
            return text.to_string();
        }
        format!("\x1b[{code}m{text}\x1b[0m")
    }

    pub fn info(&self, msg: &str) {
        if !self.silent {
            eprintln!("{}  {}", self.color("36", "[INFO]"), msg);
        }
    }

    pub fn success(&self, msg: &str) {
        if !self.silent {
            eprintln!("{}   {}", self.color("32", "[OK]"), msg);
        }
    }

    pub fn warn(&self, msg: &str) {
        if !self.silent {
            eprintln!("{}  {}", self.color("33", "[WARN]"), msg);
        }
    }

    pub fn debug(&self, msg: &str) {
        if self.verbose && !self.silent {
            eprintln!("{}   {}", self.color("90", "[DBG]"), msg);
        }
    }
}
