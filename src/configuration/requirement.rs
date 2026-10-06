use {super::resource::GitHubRepository, std::fmt::Display};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tool {
    Winget,
    Cargo,
    Claude,
    Wsl,
    Git,
    Uv,
    Dart,
}

impl Tool {
    pub fn program(&self) -> &'static str {
        match self {
            Tool::Winget => "winget",
            Tool::Cargo => "cargo",
            Tool::Claude => "claude",
            Tool::Wsl => "wsl",
            Tool::Git => "git",
            Tool::Uv => "uv",
            Tool::Dart => "dart",
        }
    }
}

impl Display for Tool {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.program())
    }
}

// ADR 0004
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Requirement {
    Tool(Tool),
    DotfilesRepository(GitHubRepository),
    // ADR 0040
    CargoBinariesOnSearchPath,
    DartBinariesOnSearchPath,
}

impl Display for Requirement {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Requirement::Tool(tool) => write!(formatter, "{tool} is not on the path"),
            Requirement::DotfilesRepository(repository) => {
                write!(formatter, "{repository} has not been cloned")
            }
            Requirement::CargoBinariesOnSearchPath => {
                formatter.write_str("cargo's bin directory is missing from PATH")
            }
            Requirement::DartBinariesOnSearchPath => {
                formatter.write_str("the bin directory dart install writes to is missing from PATH")
            }
        }
    }
}
