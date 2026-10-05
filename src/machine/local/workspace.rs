use {
    crate::{
        configuration::{BinaryName, CrateName},
        machine::workspace_reading::{
            Fingerprint, InferableBinary, MemberManifest, MemberReading, MemberTree, ObjectHash,
            Revision, WorkspaceReading, inherited_dependency_paths, member_paths,
            read_member_manifest,
        },
    },
    anyhow::{Context, Result, anyhow},
    git2::{BranchType, ObjectType, Oid, Repository, Tree},
    std::{
        collections::{BTreeMap, BTreeSet},
        path::Path,
    },
    workspace_lock::{WorkspaceLock, manifest_without_membership},
};

struct MemberAtRevision {
    fingerprint: Fingerprint,
    binaries: BTreeSet<BinaryName>,
}

pub fn read(repository_path: &Path) -> Result<Option<WorkspaceReading>> {
    if !repository_path.join(".git").exists() {
        return Ok(None);
    }

    let repository = open(repository_path)?;
    let revision = tracked_remote_revision(&repository)?;
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
) -> Result<BTreeMap<CrateName, Fingerprint>> {
    let repository = open(repository_path)?;

    Ok(members_at(&repository, revision)?
        .into_iter()
        .map(|(crate_name, member)| (crate_name, member.fingerprint))
        .collect())
}

fn open(repository_path: &Path) -> Result<Repository> {
    Repository::open(repository_path).with_context(|| {
        format!(
            "Could not open the repository at {}",
            repository_path.display()
        )
    })
}

fn tracked_remote_revision(repository: &Repository) -> Result<Revision> {
    let head = repository.head().context("its HEAD could not be read")?;
    let branch_name = head
        .shorthand()
        .ok_or_else(|| anyhow!("its checked-out revision is not a branch"))?
        .to_owned();

    let branch = repository
        .find_branch(&branch_name, BranchType::Local)
        .with_context(|| format!("{branch_name} is not a local branch"))?;
    let upstream = branch
        .upstream()
        .map_err(|_| anyhow!("its branch {branch_name} tracks no remote branch"))?;
    let commit = upstream
        .get()
        .peel_to_commit()
        .with_context(|| format!("what {branch_name} tracks does not name a commit"))?;

    Ok(Revision::from(commit.id().to_string()))
}

fn members_at(
    repository: &Repository,
    revision: &Revision,
) -> Result<BTreeMap<CrateName, MemberAtRevision>> {
    let commit = repository
        .revparse_single(revision.as_ref())
        .and_then(|object| object.peel_to_commit())
        .with_context(|| format!("{revision} is not in this clone"))?;
    let tree = commit.tree()?;

    let manifest = blob_text(repository, &tree, "Cargo.toml")?;
    let workspace_manifest = content_hash(&manifest_without_membership(&manifest)?)?;
    let lock = WorkspaceLock::read(&blob_text(repository, &tree, "Cargo.lock")?)?;
    let inherited_paths = inherited_dependency_paths(&manifest)?;

    let mut members = BTreeMap::new();
    for path in member_paths(&manifest)? {
        let crate_subtree = entry_hash(&tree, &path).ok_or_else(|| {
            anyhow!("its [workspace] member \"{path}\" is not in the repository at {revision}")
        })?;

        let member = read_member_manifest(&blob_text(
            repository,
            &tree,
            &format!("{path}/Cargo.toml"),
        )?)?;
        let member_tree = MemberTree {
            holds_a_main_file: entry_hash(&tree, &format!("{path}/src/main.rs")).is_some(),
            inferable_binaries: inferable_binaries_in(repository, &tree, &path)?,
        };

        let binaries = member.binaries(&member_tree);
        if !binaries.is_empty() {
            let dependency_subtrees =
                dependency_subtrees(repository, &tree, &path, &member, &inherited_paths)?;
            let lock_closure = content_hash(lock.closure_of(member.name.as_ref())?.as_str())?;
            members.insert(
                member.name,
                MemberAtRevision {
                    fingerprint: Fingerprint {
                        crate_subtree,
                        workspace_manifest: workspace_manifest.clone(),
                        lock_closure,
                        dependency_subtrees,
                    },
                    binaries,
                },
            );
        }
    }

    Ok(members)
}

fn dependency_subtrees(
    repository: &Repository,
    tree: &Tree,
    member_path: &str,
    member: &MemberManifest,
    inherited_paths: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, ObjectHash>> {
    let mut subtrees = BTreeMap::new();
    let mut pending = member.directories_depended_on(member_path, inherited_paths)?;
    while let Some(directory) = pending.pop() {
        if directory == member_path || subtrees.contains_key(&directory) {
            continue;
        }
        let subtree = entry_hash(tree, &directory).ok_or_else(|| {
            anyhow!("\"{directory}\", which \"{member_path}\" depends on, is not in the repository")
        })?;
        subtrees.insert(directory.clone(), subtree);

        let dependency = read_member_manifest(&blob_text(
            repository,
            tree,
            &format!("{directory}/Cargo.toml"),
        )?)?;
        pending.extend(dependency.directories_depended_on(&directory, inherited_paths)?);
    }
    Ok(subtrees)
}

fn inferable_binaries_in(
    repository: &Repository,
    tree: &Tree,
    member_path: &str,
) -> Result<Vec<InferableBinary>> {
    let directory = format!("{member_path}/src/bin");
    let entry = match tree.get_path(Path::new(&directory)) {
        Ok(entry) => entry,
        Err(error) if error.code() == git2::ErrorCode::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(error).with_context(|| format!("{directory} could not be read"));
        }
    };
    let entries = repository
        .find_tree(entry.id())
        .with_context(|| format!("{directory} is not a directory"))?;

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

fn entry_hash(tree: &Tree, path: &str) -> Option<ObjectHash> {
    tree.get_path(Path::new(path))
        .ok()
        .map(|entry| ObjectHash::from(entry.id().to_string()))
}

fn content_hash(content: &str) -> Result<ObjectHash> {
    let object = Oid::hash_object(ObjectType::Blob, content.as_bytes())?;
    Ok(ObjectHash::from(object.to_string()))
}

fn blob_text(repository: &Repository, tree: &Tree, path: &str) -> Result<String> {
    let entry = tree
        .get_path(Path::new(path))
        .with_context(|| format!("it holds no {path}"))?;
    let blob = repository
        .find_blob(entry.id())
        .with_context(|| format!("{path} is not a file"))?;

    String::from_utf8(blob.content().to_vec()).with_context(|| format!("{path} is not text"))
}
