Feature: Planning what a machine needs

  A change set says what would change, what is already true, and what cannot be read yet.
  Producing one never changes the machine it describes.

  Scenario: A declared application that is not installed is reported as a change
    Given Alice declares the application "Neovim"
    And Neovim is not installed on Alice's machine
    When Alice plans
    Then the change set reports 1 change
    And the change set mentions "Neovim"

  Scenario: A declared application that is already installed is reported as converged
    Given Alice declares the application "Neovim"
    And Neovim is installed on Alice's machine
    When Alice plans
    Then the change set reports 0 changes
    And the change set reports the machine as converged

  Scenario: A released binary the machine does not have is reported as a change
    Given Alice declares the released binary "rg.exe" from "BurntSushi/ripgrep"
    And the latest release of "BurntSushi/ripgrep" is "v15.1.0"
    When Alice plans
    Then the change set reports 1 change
    And the change set mentions "15.1.0"

  Scenario: A released binary at the version of the latest release is reported as converged
    Given Alice declares the released binary "rg.exe" from "BurntSushi/ripgrep"
    And the latest release of "BurntSushi/ripgrep" is "v15.1.0"
    And "rg.exe" is installed and reports "ripgrep 15.1.0"
    When Alice plans
    Then the change set reports 0 changes
    And the change set reports the machine as converged

  Scenario: A released binary behind the latest release is a change naming both versions
    Given Alice declares the released binary "rg.exe" from "BurntSushi/ripgrep"
    And the latest release of "BurntSushi/ripgrep" is "v15.1.0"
    And "rg.exe" is installed and reports "ripgrep 14.0.0"
    When Alice plans
    Then the change set reports 1 change
    And the change set mentions "14.0.0"
    And the change set mentions "15.1.0"

  Scenario: A binary printing its version somewhere else than declared is refused, not misread
    Given Alice declares the released binary "rg.exe" from "BurntSushi/ripgrep"
    And the latest release of "BurntSushi/ripgrep" is "v15.1.0"
    And "rg.exe" is installed and reports "ripgrep version 15.1.0"
    When Alice plans
    Then the change set reports 0 changes
    And the change set reports 1 blocked resource
    And the change set mentions "not a version"
    And the change set does not report the machine as converged

  Scenario: A released binary printing its version as a key and a value is read as the value
    Given Alice declares the released binary "rg.exe" from "BurntSushi/ripgrep"
    And the latest release of "BurntSushi/ripgrep" is "v15.1.0"
    And "rg.exe" is installed and reports "ripgrep version=15.1.0,"
    When Alice plans
    Then the change set reports 0 changes
    And the change set reports the machine as converged

  Scenario: A released binary from a repository that has published nothing is a change
    Given Alice declares the released binary "rg.exe" from "BurntSushi/ripgrep"
    When Alice plans
    Then the change set reports 1 change
    And the change set mentions "no release"

  Scenario: A build behind its own latest release is a change no configuration declared
    Given a newer configurator than this machine holds has been released
    When Alice plans
    Then the change set reports 1 change
    And the change set mentions "dotfiles_configurator"

  Scenario: A build at its own latest release is reported as converged
    When Alice plans
    Then the change set reports 0 changes
    And the change set reports the machine as converged

  Scenario: A declared package that the manager already holds is reported as converged
    Given Alice declares the winget package "Microsoft.PowerShell"
    And winget holds "Microsoft.PowerShell" on Alice's machine
    When Alice plans
    Then the change set reports 0 changes
    And the change set reports the machine as converged

  Scenario: A declared package the manager does not hold is reported as a change
    Given Alice declares the winget package "Microsoft.PowerShell"
    When Alice plans
    Then the change set reports 1 change

  Scenario: One package the manager holds does not make another one look held
    Given Alice declares the winget package "Microsoft.PowerShell"
    And Alice declares the winget package "Git.Git"
    And winget holds "Microsoft.PowerShell" on Alice's machine
    When Alice plans
    Then the change set reports 1 change
    And the change set mentions "Git.Git"

  Scenario: A package whose manager is absent is reported as blocked rather than as drift
    Given Alice declares the winget package "Microsoft.PowerShell"
    And winget is absent from Alice's machine
    When Alice plans
    Then the change set reports 0 changes
    And the change set reports 1 blocked resource
    And the change set does not report the machine as converged

  Scenario: A notice is reported whether or not anything has drifted
    Given Alice declares the notice "Sync the Android Studio settings repository"
    When Alice plans
    Then the change set reports 0 changes
    And the change set mentions "Sync the Android Studio settings repository"

  Scenario: An application is reported before the symlink that writes into its directory
    Given Alice declares the symlink "vscode/settings.json" at "settings.json"
    And Alice declares the application "Visual Studio Code"
    And the dotfiles repository has been cloned on Alice's machine
    And Visual Studio Code is not installed on Alice's machine
    When Alice plans
    Then the change set lists the application before the symlink

  Scenario: Planning twice against one machine produces the same change set
    Given Alice declares the application "Neovim"
    And Alice declares the application "Discord"
    And Neovim is not installed on Alice's machine
    And Discord is not installed on Alice's machine
    When Alice plans twice
    Then both change sets are the same

  Scenario: Planning leaves the machine exactly as it was
    Given Alice declares the application "Neovim"
    And Alice declares the winget package "Microsoft.PowerShell"
    And Neovim is not installed on Alice's machine
    When Alice plans
    Then nothing on Alice's machine has changed

  Scenario: A command without a presence check is reported as changing on every run
    Given Alice declares the command "refresh-completions" with no presence check
    When Alice plans
    Then the change set reports 1 change

  Scenario: A workspace crate whose content matches the repository is converged
    Given Alice declares the cargo workspace in the dotfiles repository
    And the dotfiles repository has been cloned on Alice's machine
    And the workspace holds the crate "stop-gate"
    And cargo installed "stop-gate" from the content the workspace holds now
    When Alice plans
    Then the change set reports 0 changes
    And the change set reports the machine as converged

  Scenario: A workspace crate whose content differs from the repository is a change
    Given Alice declares the cargo workspace in the dotfiles repository
    And the dotfiles repository has been cloned on Alice's machine
    And the workspace holds the crate "stop-gate"
    And cargo installed "stop-gate" from content the workspace has since changed
    When Alice plans
    Then the change set reports 1 change
    And the change set mentions "stop-gate"

  Scenario: A workspace crate cargo has never installed is a change
    Given Alice declares the cargo workspace in the dotfiles repository
    And the dotfiles repository has been cloned on Alice's machine
    And the workspace holds the crate "stop-gate"
    When Alice plans
    Then the change set reports 1 change
    And the change set mentions "stop-gate"

  Scenario: A workspace crate cargo records as installed is a change once its binary is gone
    Given Alice declares the cargo workspace in the dotfiles repository
    And the dotfiles repository has been cloned on Alice's machine
    And the workspace holds the crate "session-mining"
    And cargo installed "session-mining" from the content the workspace holds now
    And the binary "tool-use-statistics" of "session-mining" is gone from where cargo installs it
    When Alice plans
    Then the change set reports 1 change
    And the change set mentions "tool-use-statistics"

  Scenario: A crate added to the workspace is planned without the configuration changing
    Given Alice declares the cargo workspace in the dotfiles repository
    And the dotfiles repository has been cloned on Alice's machine
    And the workspace holds the crate "stop-gate"
    And the workspace holds the crate "claude-statusline"
    When Alice plans
    Then the change set reports 2 changes
    And the change set mentions "claude-statusline"

  Scenario: A workspace whose repository is not cloned yet contributes no crates
    Given Alice declares the cargo workspace in the dotfiles repository
    And the workspace holds the crate "stop-gate"
    And the dotfiles repository has not been cloned on Alice's machine
    When Alice plans
    Then the change set reports 0 changes

  Scenario: A workspace crate is blocked without git, which is what fetches the repository it installs from
    Given Alice declares the cargo workspace in the dotfiles repository
    And the dotfiles repository has been cloned on Alice's machine
    And the workspace holds the crate "stop-gate"
    And git is absent from Alice's machine
    When Alice plans
    Then the change set reports 0 changes
    And the change set reports 1 blocked resource
    And the change set does not report the machine as converged

  Scenario: A crate from the registry needs no git, since nothing is fetched to install it
    Given Alice declares the cargo package "ripgrep"
    And git is absent from Alice's machine
    When Alice plans
    Then the change set reports 1 change

  Scenario: An environment variable the machine does not set is reported as a change
    Given Alice declares the environment variable "EDITOR" as "nvim"
    When Alice plans
    Then the change set reports 1 change
    And the change set mentions "EDITOR"

  Scenario: An environment variable already set to the declared value is reported as converged
    Given Alice declares the environment variable "EDITOR" as "nvim"
    And "EDITOR" is set to "nvim" on Alice's machine
    When Alice plans
    Then the change set reports 0 changes
    And the change set reports the machine as converged

  Scenario: An environment variable set to something else is a change naming what it is set to
    Given Alice declares the environment variable "EDITOR" as "nvim"
    And "EDITOR" is set to "notepad" on Alice's machine
    When Alice plans
    Then the change set reports 1 change
    And the change set mentions "notepad"

  Scenario: A directory absent from the search path is reported as a change
    Given Alice declares the search path entry "tools/bin" under her home directory
    When Alice plans
    Then the change set reports 1 change
    And the change set mentions "tools"

  Scenario: A directory already on Alice's own search path is reported as converged
    Given Alice declares the search path entry "tools/bin" under her home directory
    And "tools/bin" is already on Alice's own search path
    When Alice plans
    Then the change set reports 0 changes
    And the change set reports the machine as converged

  Scenario: A directory the machine-wide search path carries is reported as converged
    Given Alice declares the search path entry "tools/bin" under her home directory
    And "tools/bin" is already on the machine-wide search path
    When Alice plans
    Then the change set reports 0 changes
    And the change set reports the machine as converged

  Scenario: The directory this program installs binaries into is planned without being declared
    Given nothing is on Alice's search path
    When Alice plans
    Then the change set reports 1 change
    And the change set mentions "the directory this program installs binaries into"
