#![allow(clippy::disallowed_macros)]

use {
    anyhow::Result,
    dotfiles_configurator::{
        configuration::{
            DartPackage, DartPackageName, DartReference, DartSource, GitCommit, GitRemoteUrl,
            RepositorySubdirectory,
        },
        machine::{
            CommandOutput, Exited,
            dart_reading::{DartDrift, DartLocations, DartReading},
            local::dart,
        },
    },
    std::{
        fs,
        path::{Path, PathBuf},
        process::Command,
    },
    tempfile::TempDir,
};

const PACKAGE: &str = "coderabbit_findings";
const SUBDIRECTORY: &str = "tools/coderabbit-findings";
const EXECUTABLES: [&str; 2] = ["findings", "findings-server"];
const DECLARING_TWO_EXECUTABLES: &str = "name: coderabbit_findings\nversion: 1.0.0\n\
                                         executables:\n  findings:\n  findings-server: server\n";

fn git_in(directory: &Path, arguments: &[&str]) -> CommandOutput {
    let output = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args([
            "-c",
            "user.name=Alice",
            "-c",
            "user.email=alice@example.invalid",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(arguments)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .expect("git runs");
    CommandOutput {
        exited: Exited::from(output.status),
        standard_output: String::from_utf8_lossy(&output.stdout).into_owned(),
        standard_error: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

fn succeeding(directory: &Path, arguments: &[&str]) -> String {
    let output = git_in(directory, arguments);
    assert!(
        output.exited.succeeded(),
        "git {arguments:?} failed: {}",
        output.standard_error
    );
    output.standard_output.trim().to_owned()
}

fn forward_slashed(path: &Path) -> String {
    path.display().to_string().replace('\\', "/")
}

struct Origin {
    directory: TempDir,
}

impl Origin {
    fn holding(files: &[(&str, &str)]) -> Self {
        let origin = Self {
            directory: tempfile::tempdir().expect("a directory for the origin"),
        };
        succeeding(origin.path(), &["init"]);
        origin.commit(files);
        origin
    }

    fn path(&self) -> &Path {
        self.directory.path()
    }

    fn url(&self) -> GitRemoteUrl {
        GitRemoteUrl::from(forward_slashed(self.path()))
    }

    fn commit(&self, files: &[(&str, &str)]) -> GitCommit {
        for (relative, contents) in files {
            let path = self.path().join(relative);
            fs::create_dir_all(path.parent().expect("a file sits in a directory"))
                .expect("the file's directory is creatable");
            fs::write(path, contents).expect("the file is writable");
        }
        succeeding(self.path(), &["add", "--all"]);
        succeeding(self.path(), &["commit", "--quiet", "--message", "a change"]);
        self.head()
    }

    fn head(&self) -> GitCommit {
        GitCommit::try_from(succeeding(self.path(), &["rev-parse", "HEAD"]).as_str())
            .expect("git names a full commit")
    }

    fn tag_annotated(&self, name: &str) {
        succeeding(self.path(), &["tag", "--annotate", name, "--message", name]);
    }
}

struct Machine {
    directory: TempDir,
}

impl Machine {
    fn new() -> Self {
        Self {
            directory: tempfile::tempdir().expect("a directory for the machine"),
        }
    }

    fn locations(&self) -> DartLocations {
        DartLocations {
            install_directory: self.directory.path().join("Dart").join("install"),
            pub_cache_directory: self.directory.path().join("Pub").join("Cache"),
        }
    }

    fn mirror(&self, origin: &Origin, directory_name: &str) -> PathBuf {
        let mirrors = self.locations().mirrors();
        fs::create_dir_all(&mirrors).expect("the mirrors directory is creatable");
        let url = forward_slashed(origin.path());
        succeeding(
            &mirrors,
            &["clone", "--quiet", "--mirror", url.as_str(), directory_name],
        );
        mirrors.join(directory_name)
    }

    fn bundle(&self, commit: &GitCommit, lock: &str) {
        self.bundle_declaring(commit, lock, DECLARING_TWO_EXECUTABLES);
        for executable in EXECUTABLES {
            self.shim(executable);
        }
    }

    fn bundle_declaring(&self, commit: &GitCommit, lock: &str, pubspec: &str) -> PathBuf {
        let bundle = self
            .locations()
            .bundles_of(&DartPackageName::from(PACKAGE))
            .join(commit.as_ref());
        fs::create_dir_all(&bundle).expect("the bundle directory is creatable");
        fs::write(bundle.join("pubspec.lock"), lock).expect("the lock is writable");
        fs::write(bundle.join("pubspec.yaml"), pubspec).expect("the pubspec is writable");
        bundle
    }

    fn shim(&self, executable: &str) {
        let shim = self.locations().shim_of(executable);
        fs::create_dir_all(shim.parent().expect("a shim sits in a directory"))
            .expect("the bin directory is creatable");
        fs::write(shim, "@ECHO OFF\r\n").expect("the shim is writable");
    }

    fn remove_shim(&self, executable: &str) {
        fs::remove_file(self.locations().shim_of(executable)).expect("the shim is removable");
    }

    fn read(&self, package: &DartPackage) -> Result<DartReading> {
        dart::read(&self.locations(), package, &|directory, arguments| {
            Ok(git_in(directory, arguments))
        })
    }
}

fn lock_installing(url: &GitRemoteUrl, commit: &GitCommit, beyond: &str) -> String {
    format!(
        "# Generated by pub\n\
         packages:\n\
         \x20 {PACKAGE}:\n\
         \x20   dependency: \"direct main\"\n\
         \x20   description:\n\
         \x20     path: \"{SUBDIRECTORY}\"\n\
         \x20     ref: main\n\
         \x20     resolved-ref: \"{commit}\"\n\
         \x20     url: \"{url}\"\n\
         \x20   source: git\n\
         \x20   version: \"1.0.0\"\n\
         \x20 args:\n\
         \x20   dependency: transitive\n\
         \x20   description:\n\
         \x20     name: args\n\
         \x20     sha256: \"0d1b6e2c9a8f7e6d5c4b3a291807f6e5d4c3b2a1908f7e6d5c4b3a2918070605\"\n\
         \x20     url: \"https://pub.dev\"\n\
         \x20   source: hosted\n\
         \x20   version: \"2.5.0\"\n\
         {beyond}\
         sdks:\n\
         \x20 dart: \">=3.5.0 <4.0.0\"\n"
    )
}

const A_PATH_DEPENDENCY: &str = "  shared_helpers:\n    dependency: transitive\n    description:\n      \
                                 path: \"../shared-helpers\"\n      relative: true\n    source: \
                                 path\n    version: \"0.1.0\"\n";

fn declared(url: GitRemoteUrl, reference: &str) -> DartPackage {
    DartPackage {
        name: DartPackageName::from(PACKAGE),
        source: DartSource::Git {
            url,
            path: RepositorySubdirectory::from(SUBDIRECTORY),
            reference: DartReference::from(reference),
        },
    }
}

fn the_package_alone() -> Origin {
    Origin::holding(&[
        (
            &format!("{SUBDIRECTORY}/pubspec.yaml"),
            "name: coderabbit_findings\n",
        ),
        ("README.md", "the first readme\n"),
    ])
}

fn installed_from_head(machine: &Machine, origin: &Origin, beyond: &str) -> GitCommit {
    let installed = origin.head();
    machine.bundle(
        &installed,
        &lock_installing(&origin.url(), &installed, beyond),
    );
    machine.mirror(origin, "coderabbit_findings-0a1b2c");
    installed
}

#[test]
fn a_package_installed_from_the_commit_its_branch_names_is_current() {
    let origin = the_package_alone();
    let machine = Machine::new();
    installed_from_head(&machine, &origin, "");

    assert_eq!(
        machine.read(&declared(origin.url(), "main")).unwrap(),
        DartReading::Current
    );
}

#[test]
fn a_package_whose_directory_is_unchanged_at_a_later_commit_is_current() {
    let origin = the_package_alone();
    let machine = Machine::new();
    installed_from_head(&machine, &origin, "");
    origin.commit(&[("README.md", "a later readme\n")]);

    assert_eq!(
        machine.read(&declared(origin.url(), "main")).unwrap(),
        DartReading::Current
    );
}

#[test]
fn a_package_whose_directory_changed_at_a_later_commit_is_behind_it() {
    let origin = the_package_alone();
    let machine = Machine::new();
    let installed = installed_from_head(&machine, &origin, "");
    let desired = origin.commit(&[(
        &format!("{SUBDIRECTORY}/lib/findings.dart"),
        "void main() {}\n",
    )]);

    assert_eq!(
        machine.read(&declared(origin.url(), "main")).unwrap(),
        DartReading::Drifted(DartDrift::AtAnotherCommit { installed, desired })
    );
}

#[test]
fn a_package_depending_on_a_path_beyond_itself_is_judged_by_its_commit_alone() {
    let origin = the_package_alone();
    let machine = Machine::new();
    let installed = installed_from_head(&machine, &origin, A_PATH_DEPENDENCY);
    let desired = origin.commit(&[("README.md", "a later readme\n")]);

    assert_eq!(
        machine.read(&declared(origin.url(), "main")).unwrap(),
        DartReading::Drifted(DartDrift::AtAnotherCommit { installed, desired })
    );
}

#[test]
fn a_bundle_without_a_copy_of_its_repository_in_the_pub_cache_has_drifted() {
    let origin = the_package_alone();
    let machine = Machine::new();
    let installed = origin.head();
    machine.bundle(&installed, &lock_installing(&origin.url(), &installed, ""));

    assert_eq!(
        machine.read(&declared(origin.url(), "main")).unwrap(),
        DartReading::Drifted(DartDrift::NoMirror)
    );
}

#[test]
fn the_copy_of_the_repository_read_is_the_one_whose_origin_is_the_declared_url() {
    let origin = the_package_alone();
    let elsewhere = Origin::holding(&[("README.md", "another repository\n")]);
    let machine = Machine::new();
    machine.mirror(&elsewhere, "a_decoy-000000");
    installed_from_head(&machine, &origin, "");
    origin.commit(&[("README.md", "a later readme\n")]);

    assert_eq!(
        machine.read(&declared(origin.url(), "main")).unwrap(),
        DartReading::Current
    );
}

#[test]
fn a_package_dart_has_no_bundle_of_has_drifted() {
    let origin = the_package_alone();

    assert_eq!(
        Machine::new()
            .read(&declared(origin.url(), "main"))
            .unwrap(),
        DartReading::Drifted(DartDrift::NoBundle)
    );
}

#[test]
fn a_package_dart_holds_two_bundles_of_has_drifted() {
    let origin = the_package_alone();
    let machine = Machine::new();
    let first = installed_from_head(&machine, &origin, "");
    let second = origin.commit(&[("README.md", "a later readme\n")]);
    machine.bundle(&second, &lock_installing(&origin.url(), &first, ""));

    assert_eq!(
        machine.read(&declared(origin.url(), "main")).unwrap(),
        DartReading::Drifted(DartDrift::SeveralBundles(2))
    );
}

#[test]
fn a_bundle_installed_from_another_repository_has_drifted() {
    let origin = the_package_alone();
    let machine = Machine::new();
    let installed = origin.head();
    let elsewhere = GitRemoteUrl::from("https://example.invalid/another.git");
    machine.bundle(&installed, &lock_installing(&elsewhere, &installed, ""));
    machine.mirror(&origin, "coderabbit_findings-0a1b2c");

    let reading = machine.read(&declared(origin.url(), "main")).unwrap();

    let DartReading::Drifted(DartDrift::InstalledFromElsewhere(source)) = reading else {
        panic!("expected a bundle from another repository to have drifted, got {reading:?}");
    };
    assert!(source.contains("another.git"), "{source}");
}

#[test]
fn a_package_kept_at_the_commit_it_was_installed_from_is_current() {
    let origin = the_package_alone();
    let machine = Machine::new();
    let installed = installed_from_head(&machine, &origin, "");
    origin.commit(&[(
        &format!("{SUBDIRECTORY}/lib/findings.dart"),
        "void main() {}\n",
    )]);

    assert_eq!(
        machine
            .read(&declared(origin.url(), installed.as_ref()))
            .unwrap(),
        DartReading::Current
    );
}

#[test]
fn a_package_kept_at_one_commit_and_installed_from_another_has_drifted() {
    let origin = the_package_alone();
    let machine = Machine::new();
    let installed = installed_from_head(&machine, &origin, "");
    let desired = origin.commit(&[("README.md", "a later readme\n")]);

    assert_eq!(
        machine
            .read(&declared(origin.url(), desired.as_ref()))
            .unwrap(),
        DartReading::Drifted(DartDrift::AtAnotherCommit { installed, desired })
    );
}

#[test]
fn a_tag_names_the_commit_it_was_made_on_rather_than_its_own_object() {
    let origin = the_package_alone();
    origin.tag_annotated("v1.0.0");
    let machine = Machine::new();
    installed_from_head(&machine, &origin, A_PATH_DEPENDENCY);

    assert_eq!(
        machine.read(&declared(origin.url(), "v1.0.0")).unwrap(),
        DartReading::Current
    );
}

#[test]
fn a_reference_origin_does_not_hold_is_unassessable_saying_so() {
    let origin = the_package_alone();
    let machine = Machine::new();
    installed_from_head(&machine, &origin, "");

    let refusal = machine
        .read(&declared(origin.url(), "no-such-branch"))
        .unwrap_err();

    assert!(
        refusal
            .to_string()
            .contains("holds no reference named no-such-branch"),
        "{refusal:#}"
    );
}

#[test]
fn a_bundle_whose_lock_cannot_be_read_is_unassessable_rather_than_drifted() {
    let origin = the_package_alone();
    let machine = Machine::new();
    machine.bundle(&origin.head(), "packages: [unclosed\n");

    assert!(machine.read(&declared(origin.url(), "main")).is_err());
}

#[test]
fn a_bundle_whose_lock_holds_no_entry_for_the_package_is_unassessable() {
    let origin = the_package_alone();
    let machine = Machine::new();
    machine.bundle(
        &origin.head(),
        "packages: {}\nsdks:\n  dart: \">=3.5.0 <4.0.0\"\n",
    );

    let refusal = machine.read(&declared(origin.url(), "main")).unwrap_err();

    assert!(refusal.to_string().contains(PACKAGE), "{refusal:#}");
}

#[test]
fn a_package_missing_the_shim_of_one_executable_it_declares_has_drifted_naming_it() {
    let origin = the_package_alone();
    let machine = Machine::new();
    installed_from_head(&machine, &origin, "");
    machine.remove_shim("findings-server");

    assert_eq!(
        machine.read(&declared(origin.url(), "main")).unwrap(),
        DartReading::Drifted(DartDrift::MissingShims(vec!["findings-server".to_owned()]))
    );
}

#[test]
fn a_package_declaring_no_executables_needs_a_shim_for_each_executable_its_bundle_compiled() {
    let origin = the_package_alone();
    let machine = Machine::new();
    let installed = origin.head();
    let bundle = machine.bundle_declaring(
        &installed,
        &lock_installing(&origin.url(), &installed, ""),
        "name: coderabbit_findings\nversion: 1.0.0\n",
    );
    let compiled = bundle.join("bundle").join("bin");
    fs::create_dir_all(&compiled).expect("the compiled directory is creatable");
    fs::write(
        compiled.join(format!("findings{}", std::env::consts::EXE_SUFFIX)),
        "",
    )
    .expect("the compiled executable is writable");
    machine.mirror(&origin, "coderabbit_findings-0a1b2c");

    assert_eq!(
        machine.read(&declared(origin.url(), "main")).unwrap(),
        DartReading::Drifted(DartDrift::MissingShims(vec!["findings".to_owned()]))
    );
}
