// ADR 0026
#![allow(clippy::disallowed_macros)]

use {
    dotfiles_configurator::{
        configuration::{
            BUILD_GENERATION, MachineClass, Migration, MigrationRefused,
            OLDEST_READABLE_GENERATION, Unreadable,
        },
        configuration_source::{
            AbsoluteDirectory, ConfigurationSource, LoadFailure, WriteSource, load_desired_state,
        },
        convergence::plan,
        desired_state::DesiredState,
        github::GitHubAccess,
        reporting::RunKind,
    },
    std::{
        env, fs,
        path::{Path, PathBuf},
        process,
    },
};

#[path = "common/declarations.rs"]
mod declarations;
#[path = "common/fake_machine.rs"]
mod fake_machine;

use {declarations::Reporting, fake_machine::FakeMachine};

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name);
    fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn a_checkout_holding(name: &str, documents: &[(&str, String)]) -> PathBuf {
    let checkout = env::temp_dir()
        .join("dotfiles_migration_tests")
        .join(process::id().to_string())
        .join(name);
    let _ = fs::remove_dir_all(&checkout);
    fs::create_dir_all(checkout.join(".git")).unwrap();
    fs::create_dir_all(checkout.join("config")).unwrap();

    for (file_name, contents) in documents {
        fs::write(checkout.join("config").join(file_name), contents).unwrap();
    }
    an_estate_keying(&checkout, r#"{ "Alice": {}, "AliceTools": {} }"#);
    checkout
}

fn an_estate_keying(checkout: &Path, owners: &str) {
    let folder = checkout.join("estates").join("personal");
    fs::create_dir_all(&folder).unwrap();
    fs::write(
        folder.join("estate.json"),
        format!(r#"{{ "owners": {owners} }}"#),
    )
    .unwrap();
}

fn a_readable_set(personal: String) -> Vec<(&'static str, String)> {
    let everywhere = format!(
        r#"{{ "version": "{BUILD_GENERATION}", "applies_to": "everywhere",
           "github_account": "Alice", "resources": [] }}"#
    );
    vec![
        ("everywhere.dotconfig.json", everywhere),
        ("personal.dotconfig.json", personal),
    ]
}

fn personal_document(checkout: &Path) -> String {
    fs::read_to_string(checkout.join("config").join("personal.dotconfig.json")).unwrap()
}

async fn attempt_to_load(checkout: &Path) -> Result<DesiredState, LoadFailure> {
    load_desired_state(
        &[ConfigurationSource::LocalDirectory(
            AbsoluteDirectory::of(checkout.join("config"))
                .expect("a temporary directory is absolute"),
        )],
        MachineClass::Personal,
        Path::new("/repositories"),
        &GitHubAccess::new(),
    )
    .await
}

async fn load(checkout: &Path) -> DesiredState {
    attempt_to_load(checkout).await.unwrap()
}

struct RealFilesystem;

impl WriteSource for RealFilesystem {
    fn rewrite(&self, migration: &Migration) -> anyhow::Result<()> {
        fs::write(migration.path(), migration.contents())?;
        Ok(())
    }
}

fn rewrite_all(migrations: &[Migration]) {
    for migration in migrations {
        RealFilesystem.rewrite(migration).unwrap();
    }
}

#[tokio::test]
async fn a_generation_11_document_is_rewritten_as_the_generation_12_document_beside_it() {
    let documents = a_readable_set(fixture("generation_11.dotconfig.json"));
    let checkout = a_checkout_holding("rewritten_document", &documents);

    let desired_state = load(&checkout).await;
    rewrite_all(&desired_state.migrations);

    assert_eq!(
        personal_document(&checkout),
        fixture("generation_12.dotconfig.json")
    );
}

#[tokio::test]
async fn a_generation_11_estate_naming_an_owner_its_estate_file_does_not_key_is_not_migrated() {
    let documents = a_readable_set(fixture("generation_11.dotconfig.json"));
    let checkout = a_checkout_holding("owner_outside_the_estate_file", &documents);
    an_estate_keying(&checkout, r#"{ "Alice": {} }"#);

    let refusal = attempt_to_load(&checkout)
        .await
        .expect_err("an owner the estate file does not key refuses the migration");

    let [unreadable] = refusal.unreadable() else {
        panic!("expected one unreadable configuration, got: {refusal}");
    };
    assert!(
        matches!(
            unreadable,
            Unreadable::Malformed(fault)
                if fault.downcast_ref::<MigrationRefused>().is_some_and(|refused| matches!(
                    refused,
                    MigrationRefused::OwnerMissingFromTheEstateFile { owner, .. }
                        if owner.as_declared() == "AliceTools"
                ))
        ),
        "{refusal}"
    );
    assert_eq!(
        personal_document(&checkout),
        fixture("generation_11.dotconfig.json")
    );
}

#[tokio::test]
async fn a_generation_11_document_reports_one_migration_naming_the_generation_it_came_from() {
    let documents = a_readable_set(fixture("generation_11.dotconfig.json"));
    let checkout = a_checkout_holding("reported_migration", &documents);

    let desired_state = load(&checkout).await;

    let [migration] = &desired_state.migrations[..] else {
        panic!("expected one migration, got {:?}", desired_state.migrations);
    };
    let reported = migration.to_string();
    assert!(
        reported.contains(&format!("generation {OLDEST_READABLE_GENERATION}"))
            && reported.contains(&format!("generation {BUILD_GENERATION}")),
        "{reported}"
    );
}

#[tokio::test]
async fn planning_reports_a_pending_migration_and_leaves_the_document_as_it_was() {
    let documents = a_readable_set(fixture("generation_11.dotconfig.json"));
    let checkout = a_checkout_holding("plan_rewrites_nothing", &documents);
    let before = personal_document(&checkout);

    let desired_state = load(&checkout).await;
    let (change_set, _) = plan(
        &desired_state,
        &FakeMachine::default(),
        Reporting::opening(RunKind::Plan).report(),
    )
    .await
    .unwrap();

    assert_eq!(change_set.migrations.len(), 1);
    assert_eq!(personal_document(&checkout), before);
}

#[tokio::test]
async fn a_document_already_at_this_generation_leaves_nothing_to_migrate() {
    let documents = a_readable_set(fixture("generation_12.dotconfig.json"));
    let checkout = a_checkout_holding("nothing_to_migrate", &documents);

    let desired_state = load(&checkout).await;

    assert!(desired_state.migrations.is_empty());
}

#[tokio::test]
async fn a_document_rewritten_once_is_read_as_the_same_desired_state_and_migrated_no_further() {
    let documents = a_readable_set(fixture("generation_11.dotconfig.json"));
    let checkout = a_checkout_holding("read_back_migrated", &documents);

    let before_the_rewrite = load(&checkout).await;
    rewrite_all(&before_the_rewrite.migrations);
    let after_the_rewrite = load(&checkout).await;

    assert_eq!(before_the_rewrite.resources, after_the_rewrite.resources);
    assert_eq!(before_the_rewrite.workspaces, after_the_rewrite.workspaces);
    assert_eq!(before_the_rewrite.notices, after_the_rewrite.notices);
    assert!(after_the_rewrite.migrations.is_empty());
}
