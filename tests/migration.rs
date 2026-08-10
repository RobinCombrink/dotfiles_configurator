// ADR 0026
#![allow(clippy::disallowed_macros)]

use {
    dotfiles_configurator::{
        configuration::{BUILD_GENERATION, MachineClass, OLDEST_READABLE_GENERATION},
        configuration_source::{ConfigurationSource, load_desired_state},
        convergence::plan,
        desired_state::DesiredState,
        reporting::RunReport,
    },
    std::{
        env, fs,
        path::{Path, PathBuf},
    },
};

#[path = "common/fake_machine.rs"]
mod fake_machine;

use fake_machine::FakeMachine;

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name);
    fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn a_checkout_holding(name: &str, documents: &[(&str, String)]) -> PathBuf {
    let checkout = env::temp_dir().join("dotfiles_migration_tests").join(name);
    let _ = fs::remove_dir_all(&checkout);
    fs::create_dir_all(checkout.join(".git")).unwrap();
    fs::create_dir_all(checkout.join("config")).unwrap();

    for (file_name, contents) in documents {
        fs::write(checkout.join("config").join(file_name), contents).unwrap();
    }
    checkout
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

async fn load(checkout: &Path) -> DesiredState {
    load_desired_state(
        &[ConfigurationSource::LocalDirectory(checkout.join("config"))],
        MachineClass::Personal,
        Path::new("/repositories"),
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn a_generation_5_document_is_rewritten_as_the_generation_6_document_beside_it() {
    let documents = a_readable_set(fixture("generation_5.dotconfig.json"));
    let checkout = a_checkout_holding("rewritten_document", &documents);

    let desired_state = load(&checkout).await;
    for migration in &desired_state.migrations {
        migration.perform().unwrap();
    }

    assert_eq!(
        personal_document(&checkout),
        fixture("generation_6.dotconfig.json")
    );
}

#[tokio::test]
async fn a_generation_5_document_reports_one_migration_naming_the_generation_it_came_from() {
    let documents = a_readable_set(fixture("generation_5.dotconfig.json"));
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
    let documents = a_readable_set(fixture("generation_5.dotconfig.json"));
    let checkout = a_checkout_holding("plan_rewrites_nothing", &documents);
    let before = personal_document(&checkout);

    let desired_state = load(&checkout).await;
    let change_set = plan(
        &desired_state,
        &FakeMachine::default(),
        &RunReport::discarded(),
    )
    .await
    .unwrap();

    assert_eq!(change_set.migrations.len(), 1);
    assert_eq!(personal_document(&checkout), before);
}

#[tokio::test]
async fn a_document_already_at_this_generation_leaves_nothing_to_migrate() {
    let documents = a_readable_set(fixture("generation_6.dotconfig.json"));
    let checkout = a_checkout_holding("nothing_to_migrate", &documents);

    let desired_state = load(&checkout).await;

    assert!(desired_state.migrations.is_empty());
}

#[tokio::test]
async fn a_document_rewritten_once_is_read_as_the_same_desired_state_and_migrated_no_further() {
    let documents = a_readable_set(fixture("generation_5.dotconfig.json"));
    let checkout = a_checkout_holding("read_back_migrated", &documents);

    let before_the_rewrite = load(&checkout).await;
    for migration in &before_the_rewrite.migrations {
        migration.perform().unwrap();
    }
    let after_the_rewrite = load(&checkout).await;

    assert_eq!(before_the_rewrite.resources, after_the_rewrite.resources);
    assert_eq!(before_the_rewrite.workspaces, after_the_rewrite.workspaces);
    assert_eq!(before_the_rewrite.notices, after_the_rewrite.notices);
    assert!(after_the_rewrite.migrations.is_empty());
}
