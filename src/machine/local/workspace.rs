use {
    crate::{
        configuration::CrateName,
        machine::workspace_reading::{
            Fingerprint, MemberReading, MemberTree, ObjectHash, Revision, WorkspaceReading,
            member_paths, read_member_manifest,
        },
    },
    anyhow::{Context, Result, anyhow},
    git2::{BranchType, Oid, Repository, Tree},
    std::{collections::BTreeMap, path::Path},
};

pub fn read(
    repository_path: &Path,
    installed: &BTreeMap<CrateName, Revision>,
) -> Result<Option<WorkspaceReading>> {
    if !repository_path.join(".git").exists() {
        return Ok(None);
    }

    let repository = Repository::open(repository_path).with_context(|| {
        format!(
            "Could not open the repository at {}",
            repository_path.display()
        )
    })?;

    let revision = tracked_remote_revision(&repository)?;
    let desired = members_at(&repository, &revision)?;

    let mut by_revision: BTreeMap<Revision, BTreeMap<CrateName, Fingerprint>> = BTreeMap::new();
    for installed_revision in installed.values() {
        if by_revision.contains_key(installed_revision) {
            continue;
        }
        if let Ok(members) = members_at(&repository, installed_revision) {
            by_revision.insert(installed_revision.clone(), members);
        }
    }

    let members = desired
        .into_iter()
        .map(|(crate_name, desired)| {
            let installed = installed
                .get(&crate_name)
                .and_then(|revision| by_revision.get(revision))
                .and_then(|members| members.get(&crate_name).cloned());
            (crate_name, MemberReading { desired, installed })
        })
        .collect();

    Ok(Some(WorkspaceReading { revision, members }))
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
) -> Result<BTreeMap<CrateName, Fingerprint>> {
    let object_id = Oid::from_str(revision.as_ref())
        .with_context(|| format!("{revision} is not a commit identifier"))?;
    let commit = repository
        .find_commit(object_id)
        .with_context(|| format!("{revision} is not in this clone"))?;
    let tree = commit.tree()?;

    let workspace_manifest = entry_hash(&tree, "Cargo.toml")
        .ok_or_else(|| anyhow!("it holds no Cargo.toml at {revision}"))?;
    let lockfile = entry_hash(&tree, "Cargo.lock")
        .ok_or_else(|| anyhow!("it holds no Cargo.lock at {revision}"))?;
    let manifest = blob_text(repository, &tree, "Cargo.toml")?;

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
            holds_a_binaries_directory: entry_hash(&tree, &format!("{path}/src/bin")).is_some(),
        };

        if member.builds_a_binary(&member_tree) {
            members.insert(
                member.name,
                Fingerprint {
                    crate_subtree,
                    workspace_manifest: workspace_manifest.clone(),
                    lockfile: lockfile.clone(),
                },
            );
        }
    }

    Ok(members)
}

fn entry_hash(tree: &Tree, path: &str) -> Option<ObjectHash> {
    tree.get_path(Path::new(path))
        .ok()
        .map(|entry| ObjectHash::from(entry.id().to_string()))
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
