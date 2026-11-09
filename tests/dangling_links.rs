#![allow(clippy::disallowed_macros)]

#[path = "common/declarations.rs"]
mod declarations;
#[path = "common/fake_machine.rs"]
mod fake_machine;

use {
    dotfiles_configurator::convergence::{Withdrawal, dangling_links},
    fake_machine::{
        FakeMachine, dotfiles_repository_path, home_directory_path, repositories_root_path,
    },
    proptest::prelude::*,
    std::{
        collections::BTreeSet,
        path::{Path, PathBuf},
    },
};

const DIRECTORIES: usize = 3;

#[derive(Debug, Clone, Copy)]
enum TargetRoot {
    DeclaredRepository,
    UndeclaredRepository,
    Elsewhere,
}

#[derive(Debug, Clone)]
struct Entry {
    directory: usize,
    declared: bool,
    is_link: bool,
    target_root: TargetRoot,
    target_exists: bool,
}

fn directory(position: usize) -> PathBuf {
    home_directory_path()
        .join(".config")
        .join(format!("directory-{position}"))
}

fn root(target_root: TargetRoot) -> PathBuf {
    match target_root {
        TargetRoot::DeclaredRepository => dotfiles_repository_path(),
        TargetRoot::UndeclaredRepository => repositories_root_path().join("Personal").join("other"),
        TargetRoot::Elsewhere => home_directory_path().join("elsewhere"),
    }
}

fn entry_strategy() -> impl Strategy<Value = Entry> {
    (
        0..DIRECTORIES,
        any::<bool>(),
        any::<bool>(),
        prop_oneof![
            Just(TargetRoot::DeclaredRepository),
            Just(TargetRoot::UndeclaredRepository),
            Just(TargetRoot::Elsewhere),
        ],
        any::<bool>(),
    )
        .prop_map(
            |(directory, declared, is_link, target_root, target_exists)| Entry {
                directory,
                declared,
                is_link,
                target_root,
                target_exists,
            },
        )
}

struct Laid {
    link_path: PathBuf,
    target: PathBuf,
    entry: Entry,
}

fn lay_out(entries: &[Entry], unreadable: &[bool], machine: &FakeMachine) -> Vec<Laid> {
    for (position, unreadable) in unreadable.iter().enumerate() {
        if *unreadable {
            machine.make_directory_unreadable(directory(position));
        }
    }

    entries
        .iter()
        .enumerate()
        .map(|(position, entry)| {
            let name = format!("entry-{position}");
            let link_path = directory(entry.directory).join(&name);
            let target = root(entry.target_root).join(&name);
            match entry.is_link {
                true => machine.add_link(link_path.clone(), target.clone()),
                false => machine.add_own_file(link_path.clone()),
            }
            if entry.target_exists {
                machine.add_own_file(target.clone());
            }
            Laid {
                link_path,
                target,
                entry: entry.clone(),
            }
        })
        .collect()
}

fn holds_a_declared_link(laid: &[Laid], directory: usize) -> bool {
    laid.iter()
        .any(|other| other.entry.declared && other.entry.directory == directory)
}

proptest! {
    #[test]
    fn a_link_is_withdrawn_exactly_when_beside_a_declared_one_dangling_into_a_declared_repository_and_undeclared(
        entries in prop::collection::vec(entry_strategy(), 0..12),
        unreadable in prop::collection::vec(any::<bool>(), DIRECTORIES),
    ) {
        let machine = FakeMachine::default();
        let laid = lay_out(&entries, &unreadable, &machine);
        let declared_links: Vec<&Path> = laid
            .iter()
            .filter(|laid| laid.entry.declared)
            .map(|laid| laid.link_path.as_path())
            .collect();
        let repositories = BTreeSet::from([dotfiles_repository_path()]);

        let withdrawn: BTreeSet<Withdrawal> =
            dangling_links(declared_links, &repositories, &machine)
                .into_iter()
                .collect();

        let expected: BTreeSet<Withdrawal> = laid
            .iter()
            .filter(|candidate| {
                let entry = &candidate.entry;
                entry.is_link
                    && !entry.declared
                    && !entry.target_exists
                    && matches!(entry.target_root, TargetRoot::DeclaredRepository)
                    && !unreadable[entry.directory]
                    && holds_a_declared_link(&laid, entry.directory)
            })
            .map(|candidate| Withdrawal::Unlink {
                link_path: candidate.link_path.clone(),
                target: candidate.target.clone(),
                repository: dotfiles_repository_path(),
            })
            .collect();
        prop_assert_eq!(withdrawn, expected);
    }
}
