use {
    schemars::JsonSchema,
    serde::{Deserialize, Serialize},
    std::{fmt::Display, str::FromStr},
};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Context {
    Everywhere,
    Personal,
    Work,
}

impl Context {
    /// ```
    /// use dotfiles::configuration::Context;
    ///
    /// assert!(Context::Personal.includes(Context::Everywhere));
    /// assert!(Context::Personal.includes(Context::Personal));
    /// assert!(!Context::Personal.includes(Context::Work));
    /// assert!(!Context::Everywhere.includes(Context::Personal));
    /// ```
    pub fn includes(self, declared: Context) -> bool {
        declared == Context::Everywhere || declared == self
    }

    fn as_written(self) -> &'static str {
        match self {
            Context::Everywhere => "everywhere",
            Context::Personal => "personal",
            Context::Work => "work",
        }
    }
}

impl Display for Context {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_written())
    }
}

impl FromStr for Context {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "everywhere" => Ok(Context::Everywhere),
            "personal" => Ok(Context::Personal),
            "work" => Ok(Context::Work),
            other => Err(format!(
                "{other:?} is no machine this tool knows; expected `everywhere`, `personal` or \
                 `work`"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_configuration_for_every_machine_applies_to_a_machine_of_any_class() {
        assert!(Context::Personal.includes(Context::Everywhere));
        assert!(Context::Work.includes(Context::Everywhere));
        assert!(Context::Everywhere.includes(Context::Everywhere));
    }

    #[test]
    fn a_configuration_for_one_class_of_machine_applies_to_no_other_class() {
        assert!(!Context::Work.includes(Context::Personal));
        assert!(!Context::Personal.includes(Context::Work));
    }

    #[test]
    fn a_machine_belonging_to_no_class_applies_only_what_is_for_every_machine() {
        assert!(!Context::Everywhere.includes(Context::Personal));
        assert!(!Context::Everywhere.includes(Context::Work));
    }

    #[test]
    fn a_machine_this_tool_does_not_know_is_rejected_with_the_ones_it_does() {
        let error = Context::from_str("laptop").unwrap_err();

        assert!(
            error.contains("everywhere") && error.contains("personal") && error.contains("work"),
            "{error}"
        );
    }

    #[test]
    fn every_context_is_written_the_way_it_is_read() {
        for context in [Context::Everywhere, Context::Personal, Context::Work] {
            assert_eq!(Context::from_str(&context.to_string()), Ok(context));
        }
    }
}
