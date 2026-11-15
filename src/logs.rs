#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Error,
    Warning,
    Success,
    Progress,
    Build,
    Detail,
    Plain,
}
impl Tone {
    pub fn class(self) -> &'static str {
        match self {
            Self::Error => "log-error",
            Self::Warning => "log-warning",
            Self::Success => "log-success",
            Self::Progress => "log-progress",
            Self::Build => "log-build",
            Self::Detail => "log-detail",
            Self::Plain => "log-plain",
        }
    }
}
pub fn tone(line: &str) -> Tone {
    let text = line.trim_start().to_lowercase();
    if text.starts_with("error")
        || text.starts_with("traceback")
        || text.contains("error:")
        || text.contains("exception:")
        || (text.starts_with("port ") && text.contains("is in use"))
        || text.starts_with("failed")
    {
        Tone::Error
    } else if text.starts_with("warning")
        || text.starts_with("warn:")
        || text.starts_with("[warning]")
        || text.starts_with("- ")
    {
        Tone::Warning
    } else if text.starts_with("running on ")
        || text.starts_with("loaded support for ")
        || text.contains("installation complete")
        || text.starts_with("server is ready")
        || text.starts_with("downloaded ")
        || text.starts_with("built ")
        || text.starts_with("installed ")
        || text.starts_with("model update complete")
        || text.starts_with("selected language models are ready")
        || text.starts_with("up to date:")
        || text.starts_with("resolved ")
        || text.starts_with("prepared ")
        || text.starts_with("audited ")
        || text.starts_with("+ ")
    {
        Tone::Success
    } else if text.starts_with("building ") || text.starts_with("preparing ") {
        Tone::Build
    } else if text.starts_with("info:argostranslate.utils:") || text.starts_with("debug:") {
        Tone::Detail
    } else if text.starts_with("downloading")
        || text.starts_with("installing")
        || text.starts_with("updating")
        || text.starts_with("starting server")
        || text.starts_with("checking selected language models")
    {
        Tone::Progress
    } else {
        Tone::Plain
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn distinguishes_waitress_warnings_model_failures_and_lifecycle_messages() {
        assert_eq!(
            tone("WARNING:waitress.queue:Task queue depth is 3"),
            Tone::Warning
        );
        assert_eq!(
            tone("RuntimeError: Unable to open file 'model.bin'"),
            Tone::Error
        );
        assert_eq!(
            tone("Port 5000 is in use. Stop the other server."),
            Tone::Error
        );
        assert_eq!(tone("Running on http://127.0.0.1:5000"), Tone::Success);
        assert_eq!(tone("Downloading English → Romanian"), Tone::Progress);
        assert_eq!(tone("Downloaded sentencepiece"), Tone::Success);
        assert_eq!(tone("Built flask-swagger==0.2.14"), Tone::Success);
        assert_eq!(tone("Building langdetect==1.0.9"), Tone::Build);
        assert_eq!(tone("INFO:argostranslate.utils:tokenized"), Tone::Detail);
        assert_eq!(tone("Found 100 models"), Tone::Plain);
    }
}
