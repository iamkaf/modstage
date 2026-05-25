pub(super) struct TomlDocument {
    text: String,
}

impl TomlDocument {
    pub(super) fn new() -> Self {
        Self {
            text: String::new(),
        }
    }

    pub(super) fn line(&mut self, line: impl AsRef<str>) {
        self.text.push_str(line.as_ref());
        self.text.push('\n');
    }

    pub(super) fn blank(&mut self) {
        self.text.push('\n');
    }

    pub(super) fn table(&mut self, name: &str) {
        self.line(format!("[{name}]"));
    }

    pub(super) fn array_table(&mut self, name: &str) {
        self.line(format!("[[{name}]]"));
    }

    pub(super) fn string(&mut self, key: &str, value: impl AsRef<str>) {
        self.line(format!("{key} = \"{}\"", toml_escape(value.as_ref())));
    }

    pub(super) fn integer(&mut self, key: &str, value: impl std::fmt::Display) {
        self.line(format!("{key} = {value}"));
    }

    pub(super) fn string_array<'a>(
        &mut self,
        key: &str,
        values: impl IntoIterator<Item = &'a String>,
    ) {
        let values = values
            .into_iter()
            .map(|value| format!("\"{}\"", toml_escape(value)))
            .collect::<Vec<_>>()
            .join(", ");
        self.line(format!("{key} = [{values}]"));
    }

    pub(super) fn finish(self) -> String {
        self.text
    }
}

pub(super) fn toml_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}
