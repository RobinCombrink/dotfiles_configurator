use {
    common::configuration::GitCloneConfig,
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

pub(crate) fn create_download_application_progress_bar() -> ProgressBar {
    ProgressBar::new(0).with_style(ProgressStyle::default_bar()
                 .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta}) {msg}")
                 .unwrap()
                 .progress_chars("#>-"))
}

pub(crate) fn create_git_clone_progress_bar(
    owner: &String,
    repo: &String,
    repository_path: PathBuf,
) -> ProgressBar {
    let finish = indicatif::ProgressFinish::WithMessage(Cow::from(format!(
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
            "[{{elapsed_precise}}] {{bar:{}.cyan/blue}} {{pos:>7}}/{{len:7}} {{msg}}",
            execution_items_count
        ))
        .unwrap()
        .progress_chars("✓▢▢"),
    );
    let progress_bar = multi_progress.add(progress_bar);

    progress_bar.set_position(0);
    progress_bar
}

pub(crate) fn create_application_download_coordinator_progress_bar(
    multi_progress: &MultiProgress,
    applications_to_download_count: usize,
) -> ProgressBar {
    let progress_bar = create_progress_bar(
        applications_to_download_count,
        format!("Downloading applications"),
        ProgressFinish::WithMessage(Cow::from(format!(
            "{} downloaded",
            applications_to_download_count
        ))),
        ProgressStyle::with_template(&format!(
            "[{{elapsed_precise}}] {{bar:{}.cyan/blue}} {{pos:>7}}/{{len:7}} {{msg}}",
            applications_to_download_count
        ))
        .unwrap()
        .progress_chars("✓▢▢"),
    );
    let progress_bar = multi_progress.add(progress_bar);

    progress_bar.set_position(0);
    progress_bar
}

pub(crate) fn create_repositories_clone_coordinator_progress_bar(
    repository_count: usize,
) -> ProgressBar {
    create_progress_bar(
        repository_count,
        format!("Cloning {} repositories", repository_count,),
        ProgressFinish::WithMessage(Cow::from(format!("{} repos downloaded", repository_count,))),
        ProgressStyle::with_template(&format!(
            "[{{elapsed_precise}}] {{bar:{}.cyan/blue}} {{pos:>7}}/{{len:7}} {{msg}}",
            repository_count,
        ))
        .unwrap()
        .progress_chars("✓▢▢"),
    )
}

pub(crate) struct ExecutionProgress<'a> {
    coordinator: &'a MultiProgress,
    download_summary: ProgressBar,
    // git_clone_coordinator: ProgressBar,
    // dotfile_coordinator: ProgressBar,
    // shell_command_coordinator: ProgressBar,
}

impl<'a> ExecutionProgress<'a> {
    pub(crate) fn intialize(
        coordinator: &'a MultiProgress,
        config: GitCloneConfig,
        execution_plan_items_lenth: usize,
    ) -> Self {
        let download_summary =
            coordinator.add(create_application_download_coordinator_progress_bar(
                &coordinator,
                execution_plan_items_lenth,
            ));

        ExecutionProgress {
            coordinator,
            download_summary,
        }
    }
}
