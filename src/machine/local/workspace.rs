use {
    crate::{
        configuration::{BinaryName, CrateName},
        machine::workspace_reading::{
            CrateListingFailure, Fingerprint, InferableBinary, MemberReading, MemberTree,
            ObjectHash, Revision, WorkspaceReadFailure, WorkspaceReading, read_member_manifest,
        },
    },
    git2::{BranchType, Commit, ObjectType, Oid, Repository, Tree},
    std::{
        collections::{BTreeMap, BTreeSet},
        path::{Path, PathBuf},
        sync::Arc,
    },
    workspace_lock::{BuildClosure, WorkspaceLock, inherited_by},
    workspace_members::members_of,
};

struct MemberAtRevision {
    fingerprint: Fingerprint,
    binaries: BTreeSet<BinaryName>,
}

pub fn read(repository_path: &Path) -> Result<Option<WorkspaceReading>, WorkspaceReadFailure> {
    if !repository_path.join(".git").exists() {
        return Ok(None);
    }

    let repository = open(repository_path)?;
    let revision = tracked_remote_revision(&repository, repository_path)?;
    let members = members_at(&repository, &revision)?
        .into_iter()
        .map(|(crate_name, member)| {
            (
                crate_name,
                MemberReading {
                    desired: member.fingerprint,
                    binaries: member.binaries,
                },
            )
        })
        .collect();

    Ok(Some(WorkspaceReading { revision, members }))
}

pub fn read_at(
    repository_path: &Path,
    revision: &Revision,
) -> Result<BTreeMap<CrateName, Fingerprint>, WorkspaceReadFailure> {
    let repository = open(repository_path)?;

    Ok(members_at(&repository, revision)?
        .into_iter()
        .map(|(crate_name, member)| (crate_name, member.fingerprint))
        .collect())
}

fn open(repository_path: &Path) -> Result<Repository, WorkspaceReadFailure> {
    Repository::open(repository_path).map_err(|error| WorkspaceReadFailure::CloneUnopened {
        clone: repository_path.to_path_buf(),
        error: Arc::new(error),
    })
}

fn tracked_remote_revision(
    repository: &Repository,
    clone: &Path,
) -> Result<Revision, WorkspaceReadFailure> {
    let not_a_branch = || WorkspaceReadFailure::NotABranch {
        clone: clone.to_path_buf(),
    };
    let head_unreadable = |error| WorkspaceReadFailure::HeadUnreadable {
        clone: clone.to_path_buf(),
        error: Arc::new(error),
    };
    if repository.head_detached().map_err(head_unreadable)? {
        return Err(not_a_branch());
    }
    let head = repository.head().map_err(head_unreadable)?;
    let branch_name = head.shorthand().ok_or_else(not_a_branch)?.to_owned();

    let branch = repository
        .find_branch(&branch_name, BranchType::Local)
        .map_err(|error| WorkspaceReadFailure::NoLocalBranch {
            clone: clone.to_path_buf(),
            branch: branch_name.clone(),
            error: Arc::new(error),
        })?;
    let upstream =
        branch
            .upstream()
            .map_err(|error| WorkspaceReadFailure::TracksNoRemoteBranch {
                clone: clone.to_path_buf(),
                branch: branch_name.clone(),
                error: Arc::new(error),
            })?;
    let commit = upstream.get().peel_to_commit().map_err(|error| {
        WorkspaceReadFailure::TrackedNamesNoCommit {
            clone: clone.to_path_buf(),
            branch: branch_name.clone(),
            error: Arc::new(error),
        }
    })?;

    Ok(Revision::from(commit.id().to_string()))
}

pub fn resolve_commit(
    repository_path: &Path,
    revision: &Revision,
) -> Result<(), WorkspaceReadFailure> {
    commit_at(&open(repository_path)?, revision).map(|_| ())
}

fn commit_at<'repository>(
    repository: &'repository Repository,
    revision: &Revision,
) -> Result<Commit<'repository>, WorkspaceReadFailure> {
    repository
        .revparse_single(revision.as_ref())
        .and_then(|object| object.peel_to_commit())
        .map_err(|error| WorkspaceReadFailure::RevisionNotInClone {
            revision: revision.clone(),
            error: Arc::new(error),
        })
}

fn members_at(
    repository: &Repository,
    revision: &Revision,
) -> Result<BTreeMap<CrateName, MemberAtRevision>, WorkspaceReadFailure> {
    let commit = commit_at(repository, revision)?;
    let tree = commit
        .tree()
        .map_err(|error| WorkspaceReadFailure::TreeUnreadable {
            revision: revision.clone(),
            error: Arc::new(error),
        })?;

    let manifest = blob_text(repository, &tree, "Cargo.toml")?;
    let lock = WorkspaceLock::read(&blob_text(repository, &tree, "Cargo.lock")?)
        .map_err(|error| WorkspaceReadFailure::LockRefused(Arc::new(error)))?;

    let mut members = BTreeMap::new();
    let member_paths = members_of(&manifest, |parent| crates_under(repository, &tree, parent))
        .map_err(|error| WorkspaceReadFailure::MembersRefused(Arc::new(error)))?;
    for path in member_paths {
        let crate_subtree =
            entry_hash(&tree, &path).ok_or_else(|| WorkspaceReadFailure::MemberMissing {
                member: PathBuf::from(&path),
                revision: revision.clone(),
            })?;

        let member_manifest = blob_text(repository, &tree, &format!("{path}/Cargo.toml"))?;
        let workspace_binding = workspace_binding(&manifest, &path, &member_manifest)?;
        let member = read_member_manifest(&member_manifest).map_err(|refusal| {
            WorkspaceReadFailure::MemberManifestRefused {
                member: PathBuf::from(&path),
                refusal,
            }
        })?;
        let member_tree = MemberTree {
            holds_a_main_file: entry_hash(&tree, &format!("{path}/src/main.rs")).is_some(),
            inferable_binaries: inferable_binaries_in(repository, &tree, &path)?,
        };

        let binaries = member.binaries(&member_tree);
        if !binaries.is_empty() {
            let mut closure = path_dependency_closure(repository, &tree, &path, &manifest)?;
            closure.workspace_bindings.insert(path, workspace_binding);
            let lock_closure = lock
                .closure_of(member.name.as_ref())
                .map_err(|error| WorkspaceReadFailure::LockRefused(Arc::new(error)))?;
            members.insert(
                member.name,
                MemberAtRevision {
                    fingerprint: Fingerprint {
                        crate_subtree,
                        workspace_bindings: closure.workspace_bindings,
                        lock_closure: content_hash(lock_closure.as_str())?,
                        dependency_subtrees: closure.subtrees,
                    },
                    binaries,
                },
            );
        }
    }

    Ok(members)
}

fn workspace_binding(
    workspace_manifest: &str,
    directory: &str,
    member_manifest: &str,
) -> Result<ObjectHash, WorkspaceReadFailure> {
    let binding = inherited_by(workspace_manifest, member_manifest).map_err(|error| {
        WorkspaceReadFailure::BindingRefused {
            directory: PathBuf::from(directory),
            error: Arc::new(error),
        }
    })?;
    content_hash(&binding)
}

struct PathDependencyClosure {
    subtrees: BTreeMap<String, ObjectHash>,
    workspace_bindings: BTreeMap<String, ObjectHash>,
}

fn path_dependency_closure(
    repository: &Repository,
    tree: &Tree,
    member_path: &str,
    workspace_manifest: &str,
) -> Result<PathDependencyClosure, WorkspaceReadFailure> {
    let build_closure = BuildClosure::of(workspace_manifest, member_path, |directory| {
        blob_text(repository, tree, &directory.manifest()).ok()
    })
    .map_err(|error| WorkspaceReadFailure::PathDependenciesRefused {
        member: PathBuf::from(member_path),
        error: Arc::new(error),
    })?;

    let mut closure = PathDependencyClosure {
        subtrees: BTreeMap::new(),
        workspace_bindings: BTreeMap::new(),
    };
    for reached in build_closure.directories() {
        let directory = reached.as_str();
        let subtree =
            entry_hash(tree, directory).ok_or_else(|| WorkspaceReadFailure::DependencyMissing {
                directory: PathBuf::from(directory),
                member: PathBuf::from(member_path),
            })?;
        closure.subtrees.insert(directory.to_owned(), subtree);

        let dependency_manifest = blob_text(repository, tree, &reached.manifest())?;
        closure.workspace_bindings.insert(
            directory.to_owned(),
            workspace_binding(workspace_manifest, directory, &dependency_manifest)?,
        );
    }
    Ok(closure)
}

fn inferable_binaries_in(
    repository: &Repository,
    tree: &Tree,
    member_path: &str,
) -> Result<Vec<InferableBinary>, WorkspaceReadFailure> {
    let directory = format!("{member_path}/src/bin");
    let entry = match tree.get_path(Path::new(&directory)) {
        Ok(entry) => entry,
        Err(error) if error.code() == git2::ErrorCode::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(WorkspaceReadFailure::BinariesUnreadable {
                directory: PathBuf::from(directory),
                error: Arc::new(error),
            });
        }
    };
    let entries = repository.find_tree(entry.id()).map_err(|error| {
        WorkspaceReadFailure::BinariesNotADirectory {
            directory: PathBuf::from(&directory),
            error: Arc::new(error),
        }
    })?;

    Ok(entries
        .iter()
        .filter_map(|entry| {
            let name = entry.name()?;
            match entry.kind() {
                Some(ObjectType::Blob) => name.strip_suffix(".rs").map(|stem| InferableBinary {
                    path: format!("src/bin/{name}"),
                    name: BinaryName::from(stem),
                }),
                Some(ObjectType::Tree) => entry_hash(tree, &format!("{directory}/{name}/main.rs"))
                    .map(|_| InferableBinary {
                        path: format!("src/bin/{name}/main.rs"),
                        name: BinaryName::from(name),
                    }),
                _ => None,
            }
        })
        .collect())
}

fn crates_under(
    repository: &Repository,
    tree: &Tree,
    parent: &str,
) -> Result<Vec<String>, CrateListingFailure> {
    let entry = match tree.get_path(Path::new(parent)) {
        Ok(entry) => entry,
        Err(error) if error.code() == git2::ErrorCode::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(CrateListingFailure::Unread(error)),
    };
    let children = repository
        .find_tree(entry.id())
        .map_err(CrateListingFailure::Unread)?;

    let mut crates = Vec::new();
    for child in children.iter() {
        if child.kind() != Some(ObjectType::Tree) {
            continue;
        }
        let holds_a_manifest = repository
            .find_tree(child.id())
            .map_err(CrateListingFailure::Unread)?
            .get_name("Cargo.toml")
            .is_some_and(|manifest| manifest.kind() == Some(ObjectType::Blob));
        if !holds_a_manifest {
            continue;
        }
        let name = child
            .name()
            .ok_or_else(|| CrateListingFailure::NotNamedInUtf8 {
                parent: PathBuf::from(parent),
            })?;
        crates.push(name.to_owned());
    }
    Ok(crates)
}

fn entry_hash(tree: &Tree, path: &str) -> Option<ObjectHash> {
    tree.get_path(Path::new(path))
        .ok()
        .map(|entry| ObjectHash::from(entry.id().to_string()))
}

fn content_hash(content: &str) -> Result<ObjectHash, WorkspaceReadFailure> {
    let object = Oid::hash_object(ObjectType::Blob, content.as_bytes())
        .map_err(|error| WorkspaceReadFailure::ContentUnhashed(Arc::new(error)))?;
    Ok(ObjectHash::from(object.to_string()))
}

fn blob_text(
    repository: &Repository,
    tree: &Tree,
    path: &str,
) -> Result<String, WorkspaceReadFailure> {
    let entry =
        tree.get_path(Path::new(path))
            .map_err(|error| WorkspaceReadFailure::FileAbsent {
                path: PathBuf::from(path),
                error: Arc::new(error),
            })?;
    let blob =
        repository
            .find_blob(entry.id())
            .map_err(|error| WorkspaceReadFailure::NotAFile {
                path: PathBuf::from(path),
                error: Arc::new(error),
            })?;

    String::from_utf8(blob.content().to_vec()).map_err(|error| WorkspaceReadFailure::NotText {
        path: PathBuf::from(path),
        error: Arc::new(error),
    })
}
