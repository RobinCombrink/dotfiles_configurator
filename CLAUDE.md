# dotfiles_configurator

@~/.claude/stacks/rust.md

## Local rules

- **`LocalMachine` is constructed by no test**: `LocalMachine::new` calls `dirs::download_dir()`,
  which returns `None` on a stock `ubuntu-latest` runner with no `~/.config/user-dirs.dirs`
  (verified 2026-08-08 against `dirs-6.0.0` and `dirs-sys-0.5.0`), so a test constructing it passes
  on Windows and fails on CI for a reason unrelated to what it asserts. Test through `FakeMachine`
  and the `ReadMachine`/`WriteMachine` traits; an adapter-only change may honestly carry no test.
