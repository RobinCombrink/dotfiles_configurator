use {
    crate::configuration::{
        ArchiveEntry, AssetPattern, GitHubRepository, ReleasedBinary, RepositoryName,
        RepositoryOwner, VersionWord,
    },
    std::num::NonZeroUsize,
};

// ADR 0019
pub const RELEASE_OWNER: &str = "RobinCombrink";

const RELEASE_ASSET: &str = concat!(env!("CARGO_PKG_NAME"), "-windows-x86_64.zip");

const RELEASED_BINARY: &str = concat!(env!("CARGO_PKG_NAME"), ".exe");

const VERSION_WORD: usize = 2;

pub fn own_release_asset_name() -> &'static str {
    RELEASE_ASSET
}

pub fn own_release_repository() -> GitHubRepository {
    GitHubRepository {
        owner: RepositoryOwner::from(RELEASE_OWNER),
        repository: RepositoryName::from(env!("CARGO_PKG_NAME")),
    }
}

pub fn own_currency() -> ReleasedBinary {
    ReleasedBinary {
        repository: own_release_repository(),
        asset: AssetPattern::Exact(RELEASE_ASSET.to_owned()),
        entry: ArchiveEntry::try_from(RELEASED_BINARY.to_owned())
            .expect("this crate's own binary names a file"),
        version_arguments: vec!["--version".to_owned()],
        version_word: VersionWord::from(
            NonZeroUsize::new(VERSION_WORD).expect("a word position counts from one"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_release_this_build_updates_itself_from_is_named_by_the_crate_rather_than_by_hand() {
        assert_eq!(
            own_release_repository().repository,
            RepositoryName::from("dotfiles_configurator")
        );
    }

    #[test]
    fn the_binary_this_build_installs_over_itself_carries_the_name_it_runs_under() {
        assert_eq!(
            own_currency().installed_name(),
            crate::configuration::BinaryName::from("dotfiles_configurator.exe")
        );
    }

    #[test]
    fn the_version_a_released_build_reports_is_read_out_of_the_word_it_prints_it_in() {
        let printed = format!("{} 3.7.0", env!("CARGO_PKG_NAME"));

        assert_eq!(
            own_currency().reported_version(&printed).unwrap(),
            crate::version::Version::try_from("3.7.0").unwrap()
        );
    }
}
