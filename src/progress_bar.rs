use {
    indicatif::{MultiProgress, ProgressBar, ProgressFinish, ProgressStyle},
    std::{borrow::Cow, path::PathBuf, time::Duration},
};

pub(crate) fn create_progress_bar(
    length: usize,
    message: impl Into<Cow<'static, str>>,
    finish: ProgressFinish,
    style: ProgressStyle,
) -> ProgressBar {
    ProgressBar::new(length.try_into().unwrap())
        .with_finish(finish)
        .with_message(message)
        .with_style(style)
        .with_elapsed(Duration::new(0, 0))
        .with_position(0)
}

pub(crate) fn create_dotfiles_progress_bar(
    owner: &String,
    repo: &String,
    repository_path: PathBuf,
) -> ProgressBar {
    let finish = ProgressFinish::WithMessage(Cow::from(format!(
        "Cloned {owner}/{repo} into {:#?}",
        repository_path
    )));

    let message = format!("Cloning {owner}/{repo} into {:#?}", repository_path);

    let style = ProgressStyle::with_template(
        "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos:>7}/{len:7} ({eta}) {msg}",
    )
    .unwrap()
    .progress_chars("##-");

    create_progress_bar(40, message, finish, style)
}

pub(crate) fn create_execution_item_coordinator_progress_bar(
    multi_progress: &MultiProgress,
    execution_items_count: usize,
) -> ProgressBar {
    let progress_bar = create_progress_bar(
        execution_items_count,
        format!("Executing Plan"),
        ProgressFinish::WithMessage(Cow::from(format!("{} executed", execution_items_count))),
        ProgressStyle::with_template(&format!(
            "{{spinner:.green}} [{{elapsed_precise}}] {{bar:{}.cyan/blue}} {{pos:>7}}/{{len:7}} {{msg}}",
            execution_items_count
        ))
        .unwrap()
        .progress_chars("✓▢▢"),
    );
    multi_progress.add(progress_bar).with_position(0)
}
