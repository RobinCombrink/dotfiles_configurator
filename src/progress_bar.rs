use {
    indicatif::{MultiProgress, ProgressBar, ProgressFinish, ProgressStyle},
    std::{borrow::Cow, time::Duration},
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

pub(crate) fn create_execution_item_coordinator_progress_bar(
    multi_progress: &MultiProgress,
    execution_items_count: usize,
) -> ProgressBar {
    let progress_bar = create_progress_bar(
        execution_items_count,
        "Executing Plan",
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
