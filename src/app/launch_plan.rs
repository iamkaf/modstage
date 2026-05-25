use super::*;

pub(super) struct LaunchPlanBuilder {
    args: Vec<String>,
}

impl LaunchPlanBuilder {
    pub(super) fn new() -> Self {
        Self { args: Vec::new() }
    }

    pub(super) fn arg(&mut self, command: &mut Command, value: impl Into<String>) {
        let value = value.into();
        command.arg(&value);
        self.args.push(value);
    }

    pub(super) fn arg_pair(
        &mut self,
        command: &mut Command,
        key: impl Into<String>,
        value: impl Into<String>,
    ) {
        self.arg(command, key);
        self.arg(command, value);
    }

    pub(super) fn contains(&self, arg: &str) -> bool {
        self.args.iter().any(|existing| existing == arg)
    }

    pub(super) fn args(&self) -> &[String] {
        &self.args
    }
}
