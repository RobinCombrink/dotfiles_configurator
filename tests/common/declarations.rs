// Each integration test file is its own crate and pulls this module in whole, so a helper only
// one of them needs reads as dead in the others.
#![allow(dead_code)]

use {
    dotfiles_configurator::configuration::{
        ArchiveEntry, AssetPattern, GitHubRepository, ReleasedBinary, RepositoryName,
        RepositoryOwner, VersionWord,
    },
    std::num::NonZeroUsize,
};

pub fn named_repository(owner_and_name: &str) -> GitHubRepository {
    let (owner, repository) = owner_and_name
        .split_once('/')
        .expect("a repository is written owner/name");
    GitHubRepository {
        owner: RepositoryOwner::from(owner),
        repository: RepositoryName::from(repository),
    }
}

pub fn reporting_its_version_in_the_second_word(
    entry: &str,
    owner_and_name: &str,
) -> ReleasedBinary {
    ReleasedBinary {
        repository: named_repository(owner_and_name),
        asset: AssetPattern::EndsWith(".zip".to_owned()),
        entry: ArchiveEntry::try_from(entry.to_owned()).expect("an entry naming a file"),
        version_arguments: vec!["--version".to_owned()],
        version_word: VersionWord::from(NonZeroUsize::new(2).expect("a word position")),
    }
}
