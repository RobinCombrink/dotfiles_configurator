use {
    crate::{
        configuration::{Application, Package, Resource},
        desired_state::ResolvedResource,
    },
    std::fmt::Display,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Lane {
    Replacement,
    Repositories,
    Cargo,
    Install,
    Uv,
    Instant,
    Commands,
}

impl Lane {
    pub fn of(resource: &ResolvedResource) -> Self {
        if resource.replaces_the_running_build() {
            return Lane::Replacement;
        }

        match resource.declared() {
            Resource::Repository(_) => Lane::Repositories,
            Resource::Package(Package::Cargo(_)) => Lane::Cargo,
            Resource::Package(Package::Winget(_))
            | Resource::Application(Application::Installer(_)) => Lane::Install,
            Resource::Package(Package::UvTool(_)) => Lane::Uv,
            Resource::Application(Application::ReleasedBinary(_))
            | Resource::EnvironmentVariable(_)
            | Resource::Symlink(_)
            | Resource::Registration(_) => Lane::Instant,
            Resource::Command(_) => Lane::Commands,
        }
    }
}

impl Display for Lane {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.pad(match self {
            Lane::Replacement => "self",
            Lane::Repositories => "repositories",
            Lane::Cargo => "cargo",
            Lane::Install => "install",
            Lane::Uv => "uv",
            Lane::Instant => "instant",
            Lane::Commands => "commands",
        })
    }
}
