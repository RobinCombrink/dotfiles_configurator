Feature: Applying a change set

  Applying enacts a change set and keeps going until a pass changes nothing, because whether a
  resource can be converged is read from the machine rather than assumed from order.

  Scenario: Applying to a machine with no drift changes nothing
    Given Alice declares the application "Neovim"
    And Neovim is installed on Alice's machine
    When Alice applies
    Then nothing on Alice's machine has changed
    And the machine is reported as converged

  Scenario: A symlink waits for the dotfiles repository and converges once it is cloned
    Given Alice's configurations come from the dotfiles repository
    And Alice declares the symlink "gitconfig/.gitconfig" at ".gitconfig"
    And the dotfiles repository holds "gitconfig/.gitconfig"
    And the dotfiles repository has not been cloned on Alice's machine
    When Alice applies
    Then the dotfiles repository is cloned on Alice's machine
    And the link ".gitconfig" points into the dotfiles repository
    And the machine is reported as converged

  Scenario: A build behind its own latest release installs the newer one over itself
    Given a newer configurator than this machine holds has been released
    When Alice applies
    Then the configurator reports the version of its latest release
    And the machine is reported as converged

  Scenario: A released binary is installed under the name its archive entry carries
    Given Alice declares the released binary "rg.exe" from "BurntSushi/ripgrep"
    And the latest release of "BurntSushi/ripgrep" is "v15.1.0"
    When Alice applies
    Then "rg.exe" is installed in the tool directory
    And the machine is reported as converged

  Scenario: A binary whose version cannot be read is left alone rather than installed over
    Given Alice declares the released binary "rg.exe" from "BurntSushi/ripgrep"
    And the latest release of "BurntSushi/ripgrep" is "v15.1.0"
    And "rg.exe" is installed and reports "ripgrep version 15.1.0"
    When Alice applies
    Then "rg.exe" reports "ripgrep version 15.1.0"
    And 1 resource is reported as blocked
    And the machine is not reported as converged

  Scenario: A released binary behind the latest release is replaced by it
    Given Alice declares the released binary "rg.exe" from "BurntSushi/ripgrep"
    And the latest release of "BurntSushi/ripgrep" is "v15.1.0"
    And "rg.exe" is installed and reports "ripgrep 14.0.0"
    When Alice applies
    Then "rg.exe" reports "15.1.0"
    And the machine is reported as converged

  Scenario: A resource that fails to converge does not stop the ones declared after it
    Given Alice declares the application "Broken"
    And Alice declares the application "Discord"
    And Broken is not installed on Alice's machine
    And Discord is not installed on Alice's machine
    And installing Broken fails on Alice's machine
    When Alice applies
    Then Discord is installed on Alice's machine
    And 1 resource is reported as failed
    And the machine is not reported as converged

  Scenario: A resource that cannot be read leaves the machine reported as unconverged
    Given Alice declares the winget package "Microsoft.PowerShell"
    And winget is absent from Alice's machine
    When Alice applies
    Then 1 resource is reported as blocked
    And the machine is not reported as converged

  Scenario: Withdrawing a declaration leaves what it created on the machine
    Given Alice declares the application "Neovim"
    And Neovim is installed on Alice's machine
    When Alice withdraws the declaration of "Neovim"
    And Alice applies
    Then Neovim is still installed on Alice's machine
    And the machine is reported as converged

  Scenario: A failing resource is attempted once rather than again and again
    Given Alice declares the application "Broken"
    And Broken is not installed on Alice's machine
    And installing Broken fails on Alice's machine
    When Alice applies
    Then installing Broken was attempted 1 time

  Scenario: An installer that reports success without installing is not called converged
    Given Alice declares the application "Silent"
    And Silent is not installed on Alice's machine
    And installing Silent reports success without installing anything
    When Alice applies
    Then the machine is not reported as converged
    And 1 resource is reported as not having taken

  Scenario: A crate whose binary is being executed is installed over the image moved aside
    Given Alice declares the cargo workspace in the dotfiles repository
    And the dotfiles repository has been cloned on Alice's machine
    And the workspace holds the crate "claude-session"
    And Alice's machine is executing the binary "claude-session"
    When Alice applies
    Then the machine is reported as converged
    And 1 binary is superseded on Alice's machine

  Scenario: A binary that will not be moved aside is reported as held rather than as failed
    Given Alice declares the cargo workspace in the dotfiles repository
    And the dotfiles repository has been cloned on Alice's machine
    And the workspace holds the crate "session-mining"
    And Alice's machine is executing the binary "tool-use-statistics" and will not release it
    When Alice applies
    Then 1 resource is reported as held
    And 0 resources are reported as failed
    And the machine is not reported as converged

  Scenario: A held resource is attempted once rather than again on every later pass
    Given Alice declares the cargo workspace in the dotfiles repository
    And the dotfiles repository has been cloned on Alice's machine
    And the workspace holds the crate "session-mining"
    And Alice's machine is executing the binary "tool-use-statistics" and will not release it
    And Alice declares the application "Neovim"
    And Neovim is not installed on Alice's machine
    When Alice applies
    Then Neovim is installed on Alice's machine
    And cargo was asked to install 1 time

  Scenario: Applying removes the binary an earlier run moved aside
    Given Alice declares the application "Neovim"
    And Neovim is installed on Alice's machine
    And an earlier run superseded the binary "claude-session" on Alice's machine
    When Alice applies
    Then 0 binaries are superseded on Alice's machine

  Scenario: A binary still being executed survives the sweep rather than failing the run
    Given Alice declares the application "Neovim"
    And Neovim is installed on Alice's machine
    And Alice's machine is executing the binary "tool-use-statistics"
    And an earlier run superseded the binary "tool-use-statistics" on Alice's machine
    When Alice applies
    Then 1 binary is superseded on Alice's machine
    And the machine is reported as converged

  Scenario: Planning reports the binary an earlier run moved aside and removes nothing
    Given an earlier run superseded the binary "claude-session" on Alice's machine
    When Alice plans
    Then the change set mentions "claude-session"
    And 1 binary is superseded on Alice's machine

  Scenario: A real file where a link should go is reported rather than deleted
    Given Alice's configurations come from the dotfiles repository
    And Alice declares the symlink "gitconfig/.gitconfig" at ".gitconfig"
    And the dotfiles repository holds "gitconfig/.gitconfig"
    And Alice already has a file of her own at ".gitconfig"
    When Alice applies
    Then Alice's own file at ".gitconfig" is still there
    And 1 resource is reported as failed

  Scenario: A repository the work configuration declares is cloned as the work account
    Given Alice's employer's configuration declares the repository "Employer/tooling"
    When Alice applies
    Then "Employer/tooling" is cloned as "Employer"

  Scenario: Applying sets an environment variable the machine did not have
    Given Alice declares the environment variable "EDITOR" as "nvim"
    When Alice applies
    Then "EDITOR" is set to "nvim" on Alice's machine

  Scenario: Applying puts a declared directory on Alice's own search path
    Given Alice declares the search path entry "tools/bin" under her home directory
    When Alice applies
    Then "tools/bin" is on Alice's own search path

  Scenario: A directory the machine-wide search path carries is not added to Alice's own
    Given Alice declares the search path entry "tools/bin" under her home directory
    And "tools/bin" is already on the machine-wide search path
    When Alice applies
    Then "tools/bin" is not on Alice's own search path

  Scenario: A directory inside a repository reaches the search path as the path of its clone
    Given Alice declares the search path entry "bin" in the repository "flutter/flutter"
    When Alice applies
    Then "bin" inside the clone of "flutter/flutter" is on Alice's own search path

  Scenario: Applying puts the directory this program installs binaries into on the search path
    Given nothing is on Alice's search path
    When Alice applies
    Then the directory this program installs binaries into is on Alice's own search path
